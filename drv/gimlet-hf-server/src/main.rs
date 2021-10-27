//! Gimlet host flash server.
//!
//! This server is responsible for managing access to the host flash; it embeds
//! the QSPI flash driver.

#![no_std]
#![no_main]

use userlib::*;

use drv_stm32h7_gpio_api as gpio_api;
use drv_stm32h7_qspi::Qspi;
use drv_stm32h7_rcc_api as rcc_api;
use stm32h7::stm32h743 as device;

use drv_gimlet_hf_api::{HfError, InternalHfError, Operation};

declare_task!(RCC, rcc_driver);
declare_task!(GPIO, gpio_driver);

const QSPI_IRQ: u32 = 1;

#[export_name = "main"]
fn main() -> ! {
    let rcc_driver = rcc_api::Rcc::from(get_task_id(RCC));
    let gpio_driver = gpio_api::Gpio::from(get_task_id(GPIO));

    rcc_driver.enable_clock(rcc_api::Peripheral::QuadSpi);
    rcc_driver.leave_reset(rcc_api::Peripheral::QuadSpi);

    let reg = unsafe { &*device::QUADSPI::ptr() };
    let qspi = Qspi::new(reg, QSPI_IRQ);
    // Board specific goo
    cfg_if::cfg_if! {
        if #[cfg(target_board = "gimlet-1")] {
            qspi.configure(
                5, // 200MHz kernel / 5 = 40MHz clock
                25, // 2**25 = 32MiB = 256Mib
            );

            // Gimlet pin mapping
            // PF6 SP_QSPI1_IO3
            // PF7 SP_QSPI1_IO2
            // PF8 SP_QSPI1_IO0
            // PF9 SP_QSPI1_IO1
            // PF10 SP_QSPI1_CLK
            //
            // PG6 SP_QSPI1_CS
            //
            // PB2 SP_FLASH_TO_SP_RESET_L
            // PB1 SP_TO_SP3_FLASH_MUX_SELECT <-- low means us
            //
            gpio_driver.configure_alternate(
                gpio_api::Port::F.pin(6).and_pin(7).and_pin(10),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF9,
            ).unwrap();
            gpio_driver.configure_alternate(
                gpio_api::Port::F.pin(8).and_pin(9),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF10,
            ).unwrap();
            gpio_driver.configure_alternate(
                gpio_api::Port::G.pin(6),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF10,
            ).unwrap();

            // start reset and select off low
            gpio_driver.reset(gpio_api::Port::B.pin(1).and_pin(2)).unwrap();

            gpio_driver.configure_output(
                gpio_api::Port::B.pin(1).and_pin(2),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::High,
                gpio_api::Pull::None,
            ).unwrap();

            let reset_pin = gpio_api::Port::B.pin(2);
        } else if #[cfg(target_board = "gimletlet-2")] {
            qspi.configure(
                5, // 200MHz kernel / 5 = 40MHz clock
                25, // 2**25 = 32MiB = 256Mib
            );
            // Gimletlet pin mapping
            // PF6 SP_QSPI1_IO3
            // PF7 SP_QSPI1_IO2
            // PF8 SP_QSPI1_IO0
            // PF9 SP_QSPI1_IO1
            // PF10 SP_QSPI1_CLK
            //
            // PG6 SP_QSPI1_CS
            //
            // TODO check these if I have a quadspimux board
            // PF4 SP_FLASH_TO_SP_RESET_L
            // PF5 SP_TO_SP3_FLASH_MUX_SELECT <-- low means us
            //
            gpio_driver.configure_alternate(
                gpio_api::Port::F.pin(6).and_pin(7).and_pin(10),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF9,
            ).unwrap();
            gpio_driver.configure_alternate(
                gpio_api::Port::F.pin(8).and_pin(9),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF10,
            ).unwrap();
            gpio_driver.configure_alternate(
                gpio_api::Port::G.pin(6),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF10,
            ).unwrap();

            // start reset and select off low
            gpio_driver.reset(gpio_api::Port::F.pin(4).and_pin(5)).unwrap();

            gpio_driver.configure_output(
                gpio_api::Port::F.pin(4).and_pin(5),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::High,
                gpio_api::Pull::None,
            ).unwrap();

            let reset_pin = gpio_api::Port::F.pin(4);
        } else if #[cfg(target_board = "nucleo-h743zi2")] {
            qspi.configure(
                // Adjust this as needed for the SI and Logic Analyzer BW available
                200 / 25, // 200MHz kernel clock / $x MHz SPI clock = divisor
                25, // 2**25 = 32MiB = 256Mib
            );
            // Nucleo-h743zi2 pin mapping
            // There are several choices for pin assignment.
            // The CN10 connector on the board has a marked "QSPI" block
            // of pins, we use those.
            //
            // CNxx- Pin   MT25QL256xxx
            // pin   Fn    Pin           Signal   Notes
            // ----- ---   ------------, -------, ------
            // 10-07 PF4,  3,            RESET#,  10K ohm to Vcc
            // 10-09 PF5,  ---           nc,
            // 10-11 PF6,  ---           nc,
            // 10-13 PG6,  7,            CS#,     10K ohm to Vcc
            // 10-15 PB2,  16,           CLK,
            // 10-17 GND,  10,           GND,
            // 10-19 PD13, 1,            IO3,
            // 10-21 PD12, 8,            IO1,
            // 10-23 PD11, 15,           IO0,
            // 10-25 PE2,  9,            IO2,
            // 10-27 GND,  ---           nc,
            // 10-29 PA0,  ---           nc,
            // 10-31 PB0,  ---           nc,
            // 10-33 PE0,  ---           nc,
            //
            // 08-07 3V3,  2,            Vcc,     100nF to GND
            gpio_driver.configure_alternate(
                gpio_api::Port::E.pin(2),       // IO2 or nWP
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF9,
            ).unwrap();
            gpio_driver.configure_alternate(    // CLK
                gpio_api::Port::B.pin(2),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF9,
            ).unwrap();
            gpio_driver.configure_alternate(    // IO0, IO1, IO3 | nHOLD
                gpio_api::Port::D.pin(13).and_pin(12).and_pin(11),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF9,
            ).unwrap();
            gpio_driver.configure_alternate(    // nCS
                gpio_api::Port::G.pin(6),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::VeryHigh,
                gpio_api::Pull::None,
                gpio_api::Alternate::AF10,
            ).unwrap();

            // start reset and select off low
            gpio_driver.configure_output(
                gpio_api::Port::F.pin(4).and_pin(5),
                gpio_api::OutputType::PushPull,
                gpio_api::Speed::High,
                gpio_api::Pull::Up,
            ).unwrap();

            let _host_access_pin = gpio_api::Port::F.pin(5);
            let reset_pin = gpio_api::Port::F.pin(4);
        } else if #[cfg(feature = "standalone")] {
            let reset_pin = gpio_api::Port::B.pin(2);
        } else {
            compile_error!("unsupported board");
        }
    }

    // Ensure hold time for reset in case we just restarted.
    // TODO look up actual hold time requirement
    hl::sleep_for(1);

    // Release reset and let it stabilize.
    gpio_driver.set(reset_pin).unwrap();
    hl::sleep_for(10);

    // Check the ID.
    {
        let mut idbuf = [0; 20];
        qspi.read_id(&mut idbuf);

        if idbuf[0] == 0x20 && matches!(idbuf[1], 0xBA | 0xBB) {
            // ok, I believe you
        } else {
            loop {
                // We are dead now.
                hl::sleep_for(1000);
            }
        }
    }

    let mut buffer = [0; 4];
    let mut block = [0; 256];

    loop {
        hl::recv_without_notification(&mut buffer, |op, msg| match op {
            Operation::ReadId => {
                let ((), caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                let mut idbuf = [0; 20];
                qspi.read_id(&mut idbuf);

                caller.reply(idbuf);
                Ok::<_, InternalHfError>(())
            }
            Operation::ReadStatus => {
                let ((), caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                caller.reply(qspi.read_status());
                Ok::<_, InternalHfError>(())
            }
            Operation::BulkErase => {
                let ((), caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                set_and_check_write_enable(&qspi)?;
                qspi.bulk_erase();
                poll_for_write_complete(&qspi);

                caller.reply(());
                Ok::<_, InternalHfError>(())
            }
            Operation::PageProgram => {
                let (&addr, caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                let borrow = caller.borrow(0);
                let info =
                    borrow.info().ok_or(InternalHfError::MissingLease)?;

                if !info.attributes.contains(LeaseAttributes::READ) {
                    return Err(InternalHfError::BadLease);
                }
                if info.len > block.len() {
                    return Err(InternalHfError::BadLease);
                }

                // Read the entire data block into our address space.
                borrow
                    .read_fully_at(0, &mut block[..info.len])
                    .ok_or(InternalHfError::BadLease)?;

                // Now we can't fail.

                set_and_check_write_enable(&qspi)?;
                qspi.page_program(addr, &block[..info.len]);
                poll_for_write_complete(&qspi);
                caller.reply(());
                Ok::<_, InternalHfError>(())
            }
            Operation::Read => {
                let (&addr, caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                let borrow = caller.borrow(0);
                let info =
                    borrow.info().ok_or(InternalHfError::MissingLease)?;

                if !info.attributes.contains(LeaseAttributes::WRITE) {
                    return Err(InternalHfError::BadLease);
                }
                if info.len > block.len() {
                    return Err(InternalHfError::BadLease);
                }

                // addr is the flash part offset
                // length is implied by the slice of block that is given
                let mut dest_off = 0_usize;
                let dest_end = info.len;
                let mut flash_addr = addr;
                loop {
                    if dest_off == dest_end {
                        break;
                    }
                    let len = dest_end - dest_off;
                    let len = if len > block.len() {
                        block.len()
                    } else {
                        len
                    };
                    qspi.read_memory(flash_addr, &mut block[..len]);
                    borrow.write_fully_at(dest_off, &block[..len]);
                    flash_addr += len as u32;
                    dest_off += len;
                }

                caller.reply(());
                Ok::<_, InternalHfError>(())
            }
            Operation::SectorErase => {
                let (&addr, caller) =
                    msg.fixed().ok_or(InternalHfError::BadMessage)?;

                set_and_check_write_enable(&qspi)?;
                qspi.sector_erase(addr);
                poll_for_write_complete(&qspi);
                caller.reply(());
                Ok::<_, InternalHfError>(())
            }
        });
    }
}

fn set_and_check_write_enable(qspi: &Qspi) -> Result<(), HfError> {
    qspi.write_enable();
    let status = qspi.read_status();
    if status & 0b10 == 0 {
        // oh oh
        return Err(HfError::WriteEnableFailed.into());
    }
    Ok(())
}

fn poll_for_write_complete(qspi: &Qspi) {
    loop {
        let status = qspi.read_status();
        if status & 1 == 0 {
            // ooh we're done
            break;
        }
    }
}
