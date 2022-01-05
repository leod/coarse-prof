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
               | time [%]  | calls freq [Hz] | mean [ms] last [ms] min [ms] max [ms] std [ms]
frame          | 99.98     |   1e2     96.35 |     10.38     10.06    10.05    13.19     0.94
> physics      | > 3.00    |   1e1      9.64 |      3.12      3.11     3.11     3.12     0.00
> > collisions | > > 33.87 |   1e1      9.64 |      1.06      1.06     1.05     1.06     0.00
> render       | > 96.96   |   1e2     96.35 |     10.06     10.06    10.04    10.10     0.00
```
