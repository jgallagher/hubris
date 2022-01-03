// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Server task for the STM32F3/4 (currently only F4) SPI peripheral.
//!
//! Currently this hardcodes the clock rate.
//!
//! See the `spi-api` crate for the protocol being implemented here.

#![no_std]
#![no_main]

use drv_spi_api::*;
use idol_runtime::{Leased, LenLimit, RequestError, R, W};
use ringbuf::*;

#[cfg(feature = "f411")]
use stm32f4::stm32f411 as device;

use userlib::*;

use drv_stm32fx_gpio_api as gpio_api;
use drv_stm32fx_rcc_api as rcc_api;
use drv_stm32fx_spi as spi_core;

task_slot!(RCC, rcc_driver);
task_slot!(GPIO, gpio_driver);

#[derive(Copy, Clone, PartialEq)]
enum Trace {
    Start(SpiOperation, (u16, u16)),
    Tx(u16, u8),
    Rx(u16, u8),
    None,
}

ringbuf!(Trace, 64, Trace::None);

//const IRQ_MASK: u32 = 1;

#[derive(Copy, Clone, Debug)]
struct LockState {
    task: TaskId,
    device_index: usize,
}

#[export_name = "main"]
fn main() -> ! {
    check_server_config();

    let rcc_driver = rcc_api::Rcc::from(RCC.get_task_id());

    let registers = unsafe { &*CONFIG.registers };

    rcc_driver.enable_clock(CONFIG.peripheral);
    rcc_driver.leave_reset(CONFIG.peripheral);
    let mut spi = spi_core::Spi::from(registers);

    // This should correspond to '0' in the standard SPI parlance
    spi.initialize(
        device::spi1::cr1::BR_A::DIV2,
        device::spi1::cr1::DFF_A::EIGHTBIT,
        device::spi1::cr1::BIDIMODE_A::UNIDIRECTIONAL,
        device::spi1::cr1::RXONLY_A::FULLDUPLEX,
        device::spi1::cr1::LSBFIRST_A::MSBFIRST,
        device::spi1::cr1::CPHA_A::FIRSTEDGE,
        device::spi1::cr1::CPOL_A::IDLELOW,
    );

    let gpio_driver = gpio_api::Gpio::from(GPIO.get_task_id());

    // Configure all devices' CS pins to be deasserted (set).
    // We leave them in GPIO output mode from this point forward.
    for device in CONFIG.devices {
        gpio_driver
            .set_reset(device.cs.port, device.cs.pin_mask, 0)
            .unwrap();
        gpio_driver
            .configure(
                device.cs.port,
                device.cs.pin_mask,
                gpio_api::Mode::Output,
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::High,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF1, // doesn't matter in GPIO mode
            )
            .unwrap();
    }

    // Initially, configure mux 0. This keeps us from having to deal with a "no
    // mux selected" state.
    //
    // Note that the config check routine above ensured that there _is_ a mux
    // option 0.
    //
    // We deactivate before activate to avoid pin clash if we previously crashed
    // with one of these activated.
    let current_mux_index = 0;
    for opt in &CONFIG.mux_options[1..] {
        deactivate_mux_option(&opt, &gpio_driver);
    }
    activate_mux_option(&CONFIG.mux_options[current_mux_index], &gpio_driver);

    let mut server = ServerImpl {
        spi,
        gpio_driver,
        lock_holder: None,
        current_mux_index,
    };
    let mut incoming = [0u8; INCOMING_SIZE];
    loop {
        idol_runtime::dispatch(&mut incoming, &mut server);
    }
}

struct ServerImpl {
    spi: spi_core::Spi,
    gpio_driver: gpio_api::Gpio,
    lock_holder: Option<LockState>,
    current_mux_index: usize,
}

impl InOrderSpiImpl for ServerImpl {
    fn recv_source(&self) -> Option<userlib::TaskId> {
        self.lock_holder.map(|s| s.task)
    }

    fn closed_recv_fail(&mut self) {
        // Welp, someone had asked us to lock and then died. Release the
        // lock.
        self.lock_holder = None;
    }

    fn read(
        &mut self,
        _: &RecvMessage,
        device_index: u8,
        dest: LenLimit<Leased<W, [u8]>, 65535>,
    ) -> Result<(), RequestError<SpiError>> {
        self.ready_writey(SpiOperation::read, device_index, None, Some(dest))
    }
    fn write(
        &mut self,
        _: &RecvMessage,
        device_index: u8,
        src: LenLimit<Leased<R, [u8]>, 65535>,
    ) -> Result<(), RequestError<SpiError>> {
        self.ready_writey(SpiOperation::write, device_index, Some(src), None)
    }
    fn exchange(
        &mut self,
        _: &RecvMessage,
        device_index: u8,
        src: LenLimit<Leased<R, [u8]>, 65535>,
        dest: LenLimit<Leased<W, [u8]>, 65535>,
    ) -> Result<(), RequestError<SpiError>> {
        self.ready_writey(
            SpiOperation::exchange,
            device_index,
            Some(src),
            Some(dest),
        )
    }
    fn lock(
        &mut self,
        rm: &RecvMessage,
        devidx: u8,
        cs_state: CsState,
    ) -> Result<(), RequestError<SpiError>> {
        let cs_asserted = cs_state == CsState::Asserted;
        let devidx = usize::from(devidx);

        // If we are locked there are more rules:
        if let Some(lockstate) = &self.lock_holder {
            // The fact that we received this message _at all_ means
            // that the sender matched our closed receive, but just
            // in case we have a server logic bug, let's check.
            assert!(lockstate.task == rm.sender);
            // The caller is not allowed to change the device index
            // once locked.
            if lockstate.device_index != devidx {
                return Err(SpiError::BadDevice.into());
            }
        }

        // OK! We are either (1) just locking now or (2) processing
        // a legal state change from the same sender.

        // Reject out-of-range devices.
        let device = CONFIG.devices.get(devidx).ok_or(SpiError::BadDevice)?;

        // If we're asserting CS, we want to *reset* the pin. If
        // we're not, we want to *set* it. Because CS is active low.
        let pin_mask = device.cs.pin_mask;
        self.gpio_driver
            .set_reset(
                device.cs.port,
                if cs_asserted { 0 } else { pin_mask },
                if cs_asserted { pin_mask } else { 0 },
            )
            .unwrap();
        self.lock_holder = Some(LockState {
            task: rm.sender,
            device_index: devidx,
        });
        Ok(())
    }

    fn release(
        &mut self,
        rm: &RecvMessage,
    ) -> Result<(), RequestError<SpiError>> {
        if let Some(lockstate) = &self.lock_holder {
            // The fact that we were able to receive this means we
            // should be locked by the sender...but double check.
            assert!(lockstate.task == rm.sender);

            let device = &CONFIG.devices[lockstate.device_index];

            // Deassert CS. If it wasn't asserted, this is a no-op.
            // If it was, this fixes that.
            self.gpio_driver
                .set_reset(device.cs.port, device.cs.pin_mask, 0)
                .unwrap();
            self.lock_holder = None;
            Ok(())
        } else {
            Err(SpiError::NothingToRelease.into())
        }
    }
}

impl ServerImpl {
    fn ready_writey(
        &mut self,
        op: SpiOperation,
        device_index: u8,
        data_src: Option<LenLimit<Leased<R, [u8]>, 65535>>,
        data_dest: Option<LenLimit<Leased<W, [u8]>, 65535>>,
    ) -> Result<(), RequestError<SpiError>> {
        let device_index = usize::from(device_index);

        // If we are locked, check that the caller isn't mistakenly
        // addressing the wrong device.
        if let Some(lockstate) = &self.lock_holder {
            if lockstate.device_index != device_index {
                return Err(SpiError::BadDevice.into());
            }
        }

        // Reject out-of-range devices.
        let device = CONFIG
            .devices
            .get(device_index)
            .ok_or(SpiError::BadDevice)?;

        // At least one lease must be provided. A failure here indicates that
        // the server stub calling this common routine is broken, not a client
        // mistake.
        if data_src.is_none() && data_dest.is_none() {
            panic!();
        }

        // Get the required transfer lengths in the src and dest directions.
        let src_len = data_src
            .as_ref()
            .map(|leased| LenLimit::len_as_u16(&leased))
            .unwrap_or(0);
        let dest_len = data_dest
            .as_ref()
            .map(|leased| LenLimit::len_as_u16(&leased))
            .unwrap_or(0);
        let overall_len = src_len.max(dest_len);

        // Zero-byte SPI transactions don't make sense and we'll
        // decline them.
        if overall_len == 0 {
            return Err(SpiError::BadTransferSize.into());
        }

        // We have a reasonable-looking request containing reasonable-looking
        // lease(s). This is our commit point.
        ringbuf_entry!(Trace::Start(op, (src_len, dest_len)));

        // Switch the mux to the requested port.
        if device.mux_index != self.current_mux_index {
            deactivate_mux_option(
                &CONFIG.mux_options[self.current_mux_index],
                &self.gpio_driver,
            );
            activate_mux_option(
                &CONFIG.mux_options[device.mux_index],
                &self.gpio_driver,
            );
            // Remember this for later to avoid unnecessary
            // switching.
            self.current_mux_index = device.mux_index;
        }

        // Make sure SPI is on.
        self.spi.enable();

        // Enable interrupt on the conditions we're interested in.
        // TODO?
        // self.spi.enable_transfer_interrupts();

        // We're doing this! Check if we need to control CS.
        let cs_override = self.lock_holder.is_some();
        if !cs_override {
            self.gpio_driver
                .set_reset(device.cs.port, 0, device.cs.pin_mask)
                .unwrap();
        }

        // Our SPI buffer is only a single byte, so in each iteration of the
        // loop we tx 1 byte then rx 1 byte, busy-waiting if necessary. (TODO do
        // interrupts make sense with a 1-byte buffer?)
        for i in 0..overall_len {
            // Get data byte, or send 0 as a dummy value if we have none left.
            let byte = data_src.as_ref().map_or(Ok(0), |tx_data| {
                if i < src_len {
                    tx_data
                        .read_at(usize::from(i))
                        .ok_or(RequestError::went_away())
                } else {
                    Ok(0)
                }
            })?;

            // busy wait until we can tx
            while !self.spi.can_tx() {}
            ringbuf_entry!(Trace::Tx(i, byte));
            self.spi.send8(byte);

            if let Some(rx_data) = &data_dest {
                if i < dest_len {
                    // busy wait until we can rx
                    while !self.spi.can_rx() {}
                    let b = self.spi.recv8();
                    rx_data
                        .write_at(usize::from(i), b)
                        .map_err(|_| RequestError::went_away())?;
                    ringbuf_entry!(Trace::Rx(i, b));
                }
            }
        }

        // TODO? Since we tx/rx 1 byte at a time, we know we're done now and
        // don't need to wait for completion like stm32h7's driver (which uses
        // the built-in FIFOs on that chip), right?
        /*
        // Wait for the final EOT interrupt to ensure we're really
        // done before returning to the client
        loop {
            sys_irq_control(IRQ_MASK, true);
            sys_recv_closed(&mut [], IRQ_MASK, TaskId::KERNEL)
                .expect("kernel died?");

            if self.spi.check_eot() {
                self.spi.clear_eot();
                break;
            }
        }
        */

        // Check for (and clear) the rx overrun bit, which can only occur if we
        // transmitted more data than we received (i.e., `dest_len <
        // overall_len`).
        if self.spi.is_overrun() {
            // sanity check that overrun should be possible
            assert!(dest_len < overall_len);
            // clear overrun by reading DR then SR
            let _ = self.spi.recv8();
            let _ = self.spi.read_status();
            assert!(!self.spi.is_overrun());
        }

        // Deassert (set) CS.
        if !cs_override {
            self.gpio_driver
                .set_reset(device.cs.port, device.cs.pin_mask, 0)
                .unwrap();
        }

        Ok(())
    }
}

fn deactivate_mux_option(opt: &SpiMuxOption, gpio: &gpio_api::Gpio) {
    // Drive all output pins low.
    for &(pins, _af) in opt.outputs {
        gpio.set_reset(pins.port, 0, pins.pin_mask).unwrap();
        gpio.configure(
            pins.port,
            pins.pin_mask,
            gpio_api::Mode::Output,
            gpio_api::OutputType::PushPull,
            gpio_api::Speed::High,
            gpio_api::Pull::None,
            gpio_api::Alternate::AF0, // doesn't matter in GPIO mode
        )
        .unwrap();
    }
    // Switch input pin away from SPI peripheral to a GPIO input, which makes it
    // Hi-Z.
    gpio.configure(
        opt.input.0.port,
        opt.input.0.pin_mask,
        gpio_api::Mode::Input,
        gpio_api::OutputType::PushPull, // doesn't matter
        gpio_api::Speed::High,          // doesn't matter
        gpio_api::Pull::None,
        gpio_api::Alternate::AF0, // doesn't matter
    )
    .unwrap();
}

fn activate_mux_option(opt: &SpiMuxOption, gpio: &gpio_api::Gpio) {
    // Switch all outputs to the SPI peripheral.
    for &(pins, af) in opt.outputs {
        gpio.configure(
            pins.port,
            pins.pin_mask,
            gpio_api::Mode::Alternate,
            gpio_api::OutputType::PushPull,
            gpio_api::Speed::High,
            gpio_api::Pull::None,
            af,
        )
        .unwrap();
    }
    // And the input too.
    gpio.configure(
        opt.input.0.port,
        opt.input.0.pin_mask,
        gpio_api::Mode::Alternate,
        gpio_api::OutputType::PushPull, // doesn't matter
        gpio_api::Speed::High,          // doesn't matter
        gpio_api::Pull::None,
        opt.input.1,
    )
    .unwrap();
}

//////////////////////////////////////////////////////////////////////////////
// Board-peripheral-server configuration matrix
//
// The configurable bits for a given board and controller combination are in the
// ServerConfig struct. We use conditional compilation below to select _one_
// instance of this struct in a const called `CONFIG`.

/// Rolls up all the configuration options for this server on a given board and
/// controller.
#[derive(Copy, Clone)]
struct ServerConfig {
    /// Pointer to this controller's register block. Don't let the `spi1` fool
    /// you, they all have that type. This needs to match a peripheral in your
    /// task's `uses` list for this to work.
    registers: *const device::spi1::RegisterBlock,
    /// Name for the peripheral as far as the RCC is concerned.
    peripheral: rcc_api::Peripheral,
    /// We allow for an individual SPI controller to be switched between several
    /// physical sets of pads. The mux options for a given server configuration
    /// are numbered from 0 and correspond to this slice.
    mux_options: &'static [SpiMuxOption],
    /// We keep track of a fixed set of devices per SPI controller, which each
    /// have an associated routing (from `mux_options`) and CS pin.
    devices: &'static [DeviceDescriptor],
}

/// A routing of the SPI controller onto pins.
#[derive(Copy, Clone, Debug)]
struct SpiMuxOption {
    /// A list of config changes to apply to activate the output pins of this
    /// mux option. This is a list because some mux options are spread across
    /// multiple ports, or (in at least one case) the pins in the same port
    /// require different AF numbers to work.
    ///
    /// To disable the mux, we'll force these pins low. This is correct for SPI
    /// mode 0/1 but not mode 2/3; fortunately we currently don't support mode
    /// 2/3, so we can simplify.
    outputs: &'static [(PinSet, gpio_api::Alternate)],
    /// A list of config changes to apply to activate the input pins of this mux
    /// option. This is _not_ a list because there's only one such pin, CIPO.
    ///
    /// To disable the mux, we'll switch this pin to HiZ.
    input: (PinSet, gpio_api::Alternate),
}

#[derive(Copy, Clone, Debug)]
struct PinSet {
    port: gpio_api::Port,
    pin_mask: u16,
}

/// Information about one device attached to the SPI controller.
#[derive(Copy, Clone, Debug)]
struct DeviceDescriptor {
    /// To reach this device, the SPI controller has to be muxed onto the
    /// correct physical circuit. This gives the index of the right choice in
    /// the server's configured `SpiMuxOption` array.
    mux_index: usize,
    /// Where the CS pin is. While this is a `PinSet`, it should only have one
    /// pin in it, and we check this at startup.
    cs: PinSet,
}

/// Any impl of ServerConfig for Server has to pass these tests at startup.
fn check_server_config() {
    // TODO some of this could potentially be moved into const fns for building
    // the tree, and thus to compile time ... if we could assert in const fns.
    //
    // That said, because this is analyzing constants, if the checks _pass_ this
    // should disappear at compilation.

    assert!(!CONFIG.registers.is_null()); // let's start off easy.

    // Mux options must be provided.
    assert!(!CONFIG.mux_options.is_empty());
    for muxopt in CONFIG.mux_options {
        // Each mux option must contain at least one output config record.
        assert!(!muxopt.outputs.is_empty());
        let mut total_pins = 0;
        for (pinset, _af) in muxopt.outputs {
            // Each config must apply to at least one pin.
            assert!(pinset.pin_mask != 0);
            // If this is the same port as the input pin, it must not _include_
            // the input pin.
            if pinset.port == muxopt.input.0.port {
                assert!(pinset.pin_mask & muxopt.input.0.pin_mask == 0);
            }
            // We're counting how many total pins are controlled here.
            total_pins += pinset.pin_mask.count_ones();
        }
        // There should be two affected output pins (COPI, SCK). This check
        // prevents people from being clever and trying to mux SPI to two
        // locations simultaneously, which Does Not Work. It also catches
        // mistakenly including CIPO in the outputs set.
        assert!(total_pins == 2);
        // There should be exactly one pin in the input set.
        assert!(muxopt.input.0.pin_mask.count_ones() == 1);
    }
    // At least one device must be defined.
    assert!(!CONFIG.devices.is_empty());
    for dev in CONFIG.devices {
        // Mux index must be valid.
        assert!(dev.mux_index < CONFIG.mux_options.len());
        // CS pin must designate _exactly one_ pin in its mask.
        assert!(dev.cs.pin_mask.is_power_of_two());
    }
}

cfg_if::cfg_if! {
    // STM32F4V11 discovery board
    if #[cfg(all(target_board = "stm32f4-discovery", feature = "spi2"))] {
        const CONFIG: ServerConfig = ServerConfig {
            registers: device::SPI2::ptr(),
            peripheral: rcc_api::Peripheral::Spi2,
            mux_options: &[
                SpiMuxOption {
                    outputs: &[
                        (
                            PinSet {
                                port: gpio_api::Port::B,
                                pin_mask: (1 << 13) | (1 << 15),
                            },
                            gpio_api::Alternate::AF5,
                        ),
                    ],
                    input: (
                        PinSet {
                            port: gpio_api::Port::B,
                            pin_mask: 1 << 14,
                        },
                        gpio_api::Alternate::AF5,
                    ),
                },
            ],
            devices: &[
                DeviceDescriptor {
                    mux_index: 0,
                    cs: PinSet { port: gpio_api::Port::B, pin_mask: 1 << 10 },
                },
            ],
        };
    //
    // Standalone build
    //
    } else if #[cfg(feature = "standalone")] {
        // whatever - nobody gonna run it
        const CONFIG: ServerConfig = ServerConfig {
            registers: device::SPI1::ptr(),
            peripheral: rcc_api::Peripheral::Spi1,
            mux_options: &[],
            devices: &[],
        };
    } else {
        compile_error!("unsupported board-controller combination");
    }
}

include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
