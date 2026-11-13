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
        let mut bar = Self {
            display,
            backlight,
            layout: Layout::new(width, height),
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
        let (x, w) = self.layout.cell(key);
        let h = self.layout.height;
        let side = w.min(h);
        let scaled = imageops::resize(image, side, side, FilterType::Triangle);
        let mut cell = RgbImage::new(w, h);
        imageops::replace(
            &mut cell,
            &scaled,
            ((w - side) / 2) as i64,
            ((h - side) / 2) as i64,
        );
        self.display
            .draw(x, 0, w, h, |px, py| cell.get_pixel(px, py).0)
    }

    pub fn clear_key(&mut self, key: u8) -> Result<()> {
        if key >= self.layout.cols {
            return Ok(());
        }
        let (x, w) = self.layout.cell(key);
        self.display
            .draw(x, 0, w, self.layout.height, |_, _| [0; 3])
    }

    pub fn clear_all(&mut self) -> Result<()> {
        self.display
            .draw(0, 0, self.layout.width, self.layout.height, |_, _| [0; 3])
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
            let event = match touch {
                Touch::Down { .. } if was_off => continue,
                Touch::Down { slot, x } => {
                    let key = layout.key_at(if flip { 1.0 - x } else { x });
                    held.insert(slot, key);
                    KeyEvent::Down(key)
                }
                Touch::Up { slot } => match held.remove(&slot) {
                    Some(key) => KeyEvent::Up(key),
                    None => continue,
                },
            };
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
