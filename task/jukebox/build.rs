use std::{env, fs::File, io::Write, path::PathBuf};

// Low frequency notes are hurt most by rounding to u16... maybe could consider
// keeping 10*Hz instead? Probably doesn't matter with our buzzer.
fn note_to_freq(n: u8) -> u16 {
    let n = f64::from(n);
    let hz: f64 = 440.0 * 2.0_f64.powf((n - 49.0) / 12.0);
    hz.round() as u16
}

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut fh = File::create(out.join("piano.rs")).unwrap();
    writeln!(fh, "const PIANO_NOTES_HZ: [u16; 88] = [").unwrap();
    for i in 1..=88 {
        writeln!(fh, "    {},", note_to_freq(i)).unwrap();
    }
    writeln!(fh, "];").unwrap();
    fh.flush().unwrap();
}
