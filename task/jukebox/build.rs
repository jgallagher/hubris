use std::{env, fs::File, io::Write, path::PathBuf};

// Low frequency notes are hurt most by rounding to u16... maybe could consider
// keeping 10*Hz instead? Probably doesn't matter with our buzzer.
fn note_to_freq(n: u8) -> u16 {
    let n = f64::from(n);
    let hz: f64 = 440.0 * 2.0_f64.powf((n - 49.0) / 12.0);
    hz.round() as u16
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = &PathBuf::from(
        env::var_os("OUT_DIR").ok_or("missing OUT_DIR env var")?,
    );
    let mut fh = File::create(out.join("piano.rs"))?;
    writeln!(fh, "const PIANO_NOTES_HZ: [u16; 88] = [")?;
    for i in 1..=88 {
        writeln!(fh, "    {},", note_to_freq(i))?;
    }
    writeln!(fh, "];")?;
    fh.flush()?;

    idol::server::build_server_support(
        "../../idl/jukebox.idol",
        "server_stub.rs",
        idol::server::ServerStyle::InOrder,
    )?;

    Ok(())
}
