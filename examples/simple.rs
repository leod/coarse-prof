use std::thread::sleep;
use std::time::Duration;

use coarse_prof::profile;

fn render() {
    profile!("render");

    // So slow!
    sleep(Duration::from_millis(10));
}

fn main() {
    // Our game's main loop
    let num_frames = 100;
    for i in 0..num_frames {
        profile!("frame");

        // Physics don't run every frame
        if i % 10 == 0 {
            profile!("physics");
            sleep(Duration::from_millis(2));

            {
                profile!("collisions");
                sleep(Duration::from_millis(1));
            }
        }

        render();
    }

    // Print the profiling results.
    coarse_prof::write(&mut std::io::stdout()).unwrap();
}
