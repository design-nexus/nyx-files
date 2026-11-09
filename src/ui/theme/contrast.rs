// SPDX-License-Identifier: MIT

//! WCAG contrast, for lifting dim text until it reads on its backgrounds.

use super::{blend, parse_rgb_channels};

pub(super) fn readable(color: &str, backgrounds: &[&String], pole: &str, ratio: f64) -> String {
    (0..=255)
        .map(|step| blend(color, pole, f64::from(step) / 255.0))
        .find(|color| {
            backgrounds
                .iter()
                .all(|background| contrast(color, background) >= ratio)
        })
        .expect("the foreground endpoint reaches the contrast floor")
}

fn luminance(color: &str) -> f64 {
    let channels = parse_rgb_channels(color)
        .expect("validated theme color")
        .map(|channel| {
            let value = f64::from(channel) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        });
    channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
}

pub(super) fn contrast(left: &str, right: &str) -> f64 {
    let left = luminance(left);
    let right = luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}
