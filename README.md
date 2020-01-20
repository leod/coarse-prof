# coarse-prof
[![Docs Status](https://docs.rs/coarse-prof/badge.svg)](https://docs.rs/coarse-prof)
[![license](http://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/leod/coarse-prof/blob/master/LICENSE)
[![Crates.io](https://img.shields.io/crates/v/coarse-prof.svg)](https://crates.io/crates/coarse-prof)

`coarse-prof` allows you to hierarchically measure the time that blocks in
your program take, enabling you to get an intuition of where most time is
spent. This can be useful for game development, where you have a bunch of
things that need to run in every frame, such as physics, rendering,
networking and so on, and you may wish to identify the hot spots, so that
you know whether and what to optimize.

`coarse-prof`'s implementation has been inspired by
[hprof](https://github.com/cmr/hprof).
In contrast to `hprof`, which resets measurements after each frame, this
library tracks averages over multiple frames. Also, `coarse-prof` provides
the macro `profile` for profiling a scope, so that users do not have to assign a
name to scope guards.

## Usage
Just add this line to your dependencies in `Cargo.toml`:
```
coarse-prof = "0.2"
```

## Example

```rust
use std::thread::sleep;
use std::time::Duration;

use coarse_prof::profile;

fn render() {
    profile!("render");

    // So slow!
    sleep(Duration::from_millis(10));
}

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
```

Example output:
```
frame: 100.00%, 10.40ms/call @ 96.17Hz
  physics: 3.04%, 3.16ms/call @ 9.62Hz
    collisions: 33.85%, 1.07ms/call @ 9.62Hz
  render: 96.84%, 10.07ms/call @ 96.17Hz
```
