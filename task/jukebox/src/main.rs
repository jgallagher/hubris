// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![no_std]
#![no_main]

use drv_piezo_element_api::Piezo;
use userlib::*;

mod songs;

use self::songs::{Song, FUR_ELISE, TWINKLE_TWINKLE};

task_slot!(PIEZO, piezo_element);

#[export_name = "main"]
pub fn main() -> ! {
    let piezo = PIEZO.get_task_id();
    let piezo = Piezo::from(piezo);

    let mut dl = 0;
    loop {
        for song in [&FUR_ELISE, &TWINKLE_TWINKLE] {
            dl += 2000;
            hl::sleep_until(dl);
            play_song(&piezo, song, &mut dl);
        }
    }
}

fn play_song(piezo: &Piezo, song: &Song<'_>, dl: &mut u64) {
    for note in song.notes {
        let duration = note.duration.sixteenths() * song.sixteenth_ms;

        if let Some(freq) = note.freq {
            // subtract the `between_notes_ms` from the note duration so we can
            // hear a break between subsequent notes of the same pitch without
            // getting off the beat
            let on = duration - song.between_notes_ms;
            let off = song.between_notes_ms;

            piezo.piezo_on(freq).unwrap();
            *dl += on;
            hl::sleep_until(*dl);

            piezo.piezo_off().unwrap();
            *dl += off;
            hl::sleep_until(*dl);
        } else {
            // no frequency; this is a rest
            piezo.piezo_off().unwrap(); // should be unnecessary; remove?
            *dl += duration;
            hl::sleep_until(*dl);
        }
    }
}
