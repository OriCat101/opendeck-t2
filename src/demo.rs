use anyhow::Result;
use image::{Rgb, RgbImage};

use crate::bar::{self, Bar, KeyEvent};

const SIZE: u32 = 60;
const SCALE: u32 = 8;

const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

fn key_image(key: u8, cols: u8) -> RgbImage {
    let fg = [255, 255, 255];
    let bg = if key == 0 {
        [0, 110, 0]
    } else if key == cols - 1 {
        [140, 0, 0]
    } else {
        [0, 0, 0]
    };
    let mut img = RgbImage::from_pixel(SIZE, SIZE, Rgb(bg));
    let text = key.to_string();
    let glyph_w = 3 * SCALE;
    let gap = SCALE / 2;
    let total = text.len() as u32 * glyph_w + (text.len() as u32 - 1) * gap;
    let x0 = (SIZE - total) / 2;
    let y0 = (SIZE - 5 * SCALE) / 2;
    for (i, c) in text.bytes().enumerate() {
        let rows = DIGITS[(c - b'0') as usize];
        let gx = x0 + i as u32 * (glyph_w + gap);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for dy in 0..SCALE {
                    for dx in 0..SCALE {
                        img.put_pixel(gx + col * SCALE + dx, y0 + row as u32 * SCALE + dy, Rgb(fg));
                    }
                }
            }
        }
    }
    img
}

/// Hardware test without OpenDeck: numbered keys (0 green at the left end, last one red),
/// touches are printed and highlight the key.
pub async fn run() -> Result<()> {
    let mut bar = Bar::open()?;
    let layout = bar.layout();
    println!(
        "Touch Bar {}x{}, {} keys",
        layout.width, layout.height, layout.cols
    );
    bar.set_brightness(100);
    for key in 0..layout.cols {
        let image = key_image(key, layout.cols);
        if let Some(size) = bar.fit_size(key, image.dimensions()) {
            bar.set_key(key, bar::fit(&image, size));
        }
    }
    bar.flush()?;
    let (bar, mut keys) = bar::start(bar)?;
    println!("touch the keys; Ctrl-C to quit");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            event = keys.recv() => {
                let Some(event) = event else { break };
                match event {
                    KeyEvent::Down(key) => println!("down {key}"),
                    KeyEvent::Up(key) => println!("up {key}"),
                    KeyEvent::Wake => {}
                }
            }
        }
    }
    bar.lock().unwrap().shutdown();
    Ok(())
}
