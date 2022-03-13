use std::env;

use gnuplot::{Figure, AxesCommon, Caption};
use ghanima::keyboard::mouse::SpeedProfile;

// Format is just numbers separated by spaces:
//   divider delay acceleration_time  start_speed max_speed
fn parse_profiles(i: impl IntoIterator<Item = String>) -> Vec<SpeedProfile> {
    let numbers = i.into_iter()
        .map(|s| s.parse::<u16>().expect("Wrong number"))
        .collect::<Vec<_>>();

    numbers.chunks_exact(5)
        .map(|window| SpeedProfile {
            divider: window[0],
            delay: window[1],
            acceleration_time: window[2],
            start_speed: window[3],
            max_speed: window[4],
        }).collect()
}

fn main() {
    let default_profile = SpeedProfile {
        divider: 10000,
        delay: 50,
        acceleration_time: 750,
        start_speed: 5000,
        max_speed: 15000,
    };

    let args = env::args().skip(1);
    let mut profiles: Vec<(String, SpeedProfile)> = parse_profiles(args)
        .into_iter()
        .enumerate()
        .map(|(i, p)| (i.to_string(), p))
        .collect();

    if profiles.is_empty() {
        profiles.push(("Default".to_string(), default_profile));
    }

    let mut fig = Figure::new();
    for (name, profile) in &profiles {
        let t_end = ((profile.delay + profile.acceleration_time) as f32 * 1.1).ceil() as u16;
        let t = 0..t_end;
        let speed = t.clone().map(|t| profile.get_speed(t));

        fig.axes2d()
            .set_title("Speed profile", &[])
            .set_x_label("Tick", &[])
            .set_y_label("Speed", &[])
            .set_x_grid(true)
            .set_y_grid(true)
            .lines(
                t,
                speed,
                &[Caption(&name)]
            );
    }
    fig.show().unwrap();
}
