mod backlight;
mod display;
mod touch;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use image::RgbImage;
use image::imageops::{self, FilterType};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::layout::Layout;
use backlight::Backlight;
use display::Display;
use touch::{Digitizer, Touch};

const DIM_AFTER: Duration = Duration::from_secs(30);
const OFF_AFTER: Duration = Duration::from_secs(60);
const DIMMED_LEVEL: u32 = 1;

const SPACING: u32 = 16;
const INSET: u32 = 6;
const RADIUS: f32 = 8.0;
const FILL: u8 = 51;
const FILL_PRESSED: u8 = 102;

#[derive(Debug, Clone, Copy)]
pub enum KeyEvent {
    Down(u8),
    Up(u8),
}

#[derive(Clone, Copy, PartialEq)]
enum Power {
    Active,
    Dimmed,
    Off,
}

pub struct Bar {
    display: Display,
    backlight: Option<Backlight>,
    layout: Layout,
    keys: Vec<Option<RgbImage>>,
    pressed: Vec<bool>,
    image_size: Option<(u32, u32)>,
    flip: bool,
    level: u32,
    power: Power,
    last_active: Instant,
}

pub type SharedBar = Arc<Mutex<Bar>>;

impl Bar {
    pub fn open() -> Result<Self> {
        let flip = std::env::var("OPENDECK_TOUCHBAR_FLIP").is_ok_and(|v| v == "1");
        let display = Display::open(flip)?;
        let (width, height) = display.size();
        let backlight = Backlight::open()
            .inspect_err(|e| log::warn!("backlight control unavailable: {e:#}"))
            .ok();
        let level = backlight.as_ref().map_or(0, |b| b.max() / 2);
        let layout = Layout::new(width, height);
        let mut bar = Self {
            display,
            backlight,
            layout,
            keys: vec![None; layout.cols as usize],
            pressed: vec![false; layout.cols as usize],
            image_size: None,
            flip,
            level,
            power: Power::Active,
            last_active: Instant::now(),
        };
        bar.clear_all()?;
        bar.apply(Power::Active);
        Ok(bar)
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn set_key(&mut self, key: u8, image: &RgbImage) -> Result<()> {
        if key >= self.layout.cols {
            return Ok(());
        }
        let (_, w) = self.layout.cell(key);
        let (bw, bh) = self.button_size(w);
        let (iw, ih) = image.dimensions();
        if self.image_size != Some((iw, ih)) {
            self.image_size = Some((iw, ih));
            log::info!("key images are {iw}x{ih}, buttons are {bw}x{bh}");
        }
        let scale = (bw as f32 / iw.max(1) as f32).min(bh as f32 / ih.max(1) as f32);
        let fit = |v: u32| ((v as f32 * scale).round() as u32).max(1);
        self.keys[key as usize] = Some(imageops::resize(
            image,
            fit(iw),
            fit(ih),
            FilterType::Triangle,
        ));
        self.render(key)
    }

    pub fn clear_key(&mut self, key: u8) -> Result<()> {
        if key >= self.layout.cols {
            return Ok(());
        }
        self.keys[key as usize] = None;
        self.render(key)
    }

    pub fn clear_all(&mut self) -> Result<()> {
        self.keys.fill(None);
        self.display
            .draw(0, 0, self.layout.width, self.layout.height, |_, _| [0; 3])
    }

    pub fn set_pressed(&mut self, key: u8, pressed: bool) -> Result<()> {
        if key >= self.layout.cols || self.pressed[key as usize] == pressed {
            return Ok(());
        }
        self.pressed[key as usize] = pressed;
        self.render(key)
    }

    fn button_size(&self, cell_width: u32) -> (u32, u32) {
        (
            cell_width.saturating_sub(SPACING),
            self.layout.height.saturating_sub(2 * INSET),
        )
    }

    /// Draw a key as a rounded button with its image blended into the fill.
    fn render(&mut self, key: u8) -> Result<()> {
        let (x, w) = self.layout.cell(key);
        let h = self.layout.height;
        let Some(image) = &self.keys[key as usize] else {
            return self.display.draw(x, 0, w, h, |_, _| [0; 3]);
        };
        let fill = if self.pressed[key as usize] {
            FILL_PRESSED
        } else {
            FILL
        };
        let (bw, bh) = self.button_size(w);
        let (half_w, half_h) = (bw as f32 / 2.0, bh as f32 / 2.0);
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = RADIUS.min(half_w).min(half_h);
        let (iw, ih) = image.dimensions();
        let (ix, iy) = (w.saturating_sub(iw) / 2, h.saturating_sub(ih) / 2);
        self.display.draw(x, 0, w, h, |px, py| {
            let qx = ((px as f32 + 0.5 - cx).abs() - (half_w - radius)).max(0.0);
            let qy = ((py as f32 + 0.5 - cy).abs() - (half_h - radius)).max(0.0);
            let coverage = (0.5 - (qx.hypot(qy) - radius)).clamp(0.0, 1.0);
            if coverage == 0.0 {
                return [0; 3];
            }
            let mut color = [fill; 3];
            if (ix..ix + iw).contains(&px) && (iy..iy + ih).contains(&py) {
                color = image.get_pixel(px - ix, py - iy).0.map(|v| v.max(fill));
            }
            color.map(|v| (v as f32 * coverage) as u8)
        })
    }

    /// Brightness as a percentage.
    pub fn set_brightness(&mut self, percent: u8) {
        let max = self.backlight.as_ref().map_or(0, |b| b.max());
        self.level = max * percent.min(100) as u32 / 100;
        self.apply(self.power);
    }

    /// Note user activity; returns true if the bar was off and has just been woken.
    pub fn activity(&mut self) -> bool {
        let was_off = self.power == Power::Off;
        self.last_active = Instant::now();
        if self.power != Power::Active {
            self.apply(Power::Active);
        }
        was_off
    }

    pub fn tick(&mut self) {
        let idle = self.last_active.elapsed();
        let target = if idle >= OFF_AFTER {
            Power::Off
        } else if idle >= DIM_AFTER {
            Power::Dimmed
        } else {
            Power::Active
        };
        if target != self.power {
            self.apply(target);
        }
    }

    pub fn wake(&mut self) -> Result<()> {
        self.display.modeset()?;
        self.activity();
        Ok(())
    }

    pub fn shutdown(&mut self) {
        if let Err(e) = self.clear_all() {
            log::warn!("clearing the Touch Bar failed: {e:#}");
        }
        self.apply(Power::Off);
    }

    fn apply(&mut self, power: Power) {
        self.power = power;
        let value = match power {
            Power::Active => self.level,
            Power::Dimmed => DIMMED_LEVEL.min(self.level),
            Power::Off => 0,
        };
        if let Some(backlight) = &mut self.backlight
            && let Err(e) = backlight.set(value)
        {
            log::warn!("setting backlight failed: {e:#}");
        }
    }
}

/// Start touch handling and idle dimming; returns the shared bar and the stream of key events.
pub fn start(bar: Bar) -> Result<(SharedBar, UnboundedReceiver<KeyEvent>)> {
    let digitizer = Digitizer::open()?;
    let layout = bar.layout;
    let flip = bar.flip;
    let bar = Arc::new(Mutex::new(bar));

    let (touch_tx, mut touch_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        if let Err(e) = digitizer.run(touch_tx).await {
            log::error!("touch input stopped: {e:#}");
        }
    });

    let (key_tx, key_rx) = mpsc::unbounded_channel();
    let touch_bar = bar.clone();
    tokio::spawn(async move {
        let mut held: HashMap<i32, u8> = HashMap::new();
        while let Some(touch) = touch_rx.recv().await {
            let was_off = touch_bar.lock().unwrap().activity();
            let (event, key, pressed) = match touch {
                Touch::Down { .. } if was_off => continue,
                Touch::Down { slot, x } => {
                    let key = layout.key_at(if flip { 1.0 - x } else { x });
                    held.insert(slot, key);
                    (KeyEvent::Down(key), key, true)
                }
                Touch::Up { slot } => match held.remove(&slot) {
                    Some(key) => (KeyEvent::Up(key), key, false),
                    None => continue,
                },
            };
            if let Err(e) = touch_bar.lock().unwrap().set_pressed(key, pressed) {
                log::warn!("drawing key press failed: {e:#}");
            }
            if key_tx.send(event).is_err() {
                break;
            }
        }
    });

    let tick_bar = bar.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            tick_bar.lock().unwrap().tick();
        }
    });

    Ok((bar, key_rx))
}
