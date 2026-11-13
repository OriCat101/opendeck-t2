use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use evdev::{AbsoluteAxisCode, Device, EventType};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug)]
pub enum Touch {
    /// A finger landed; `x` is the position along the bar as a fraction (0 = left end).
    Down {
        slot: i32,
        x: f64,
    },
    Up {
        slot: i32,
    },
}

#[derive(Default)]
struct Slot {
    x: Option<i32>,
    pending: Option<bool>,
}

pub struct Digitizer {
    device: Device,
    min: i32,
    max: i32,
}

impl Digitizer {
    pub fn open() -> Result<Self> {
        let (path, mut device) = evdev::enumerate()
            .find(|(_, d)| d.name().is_some_and(|n| n.contains("Touch Bar")))
            .ok_or_else(|| anyhow!("no readable Touch Bar input device found"))?;
        let (_, info) = device
            .get_absinfo()?
            .find(|(axis, _)| *axis == AbsoluteAxisCode::ABS_MT_POSITION_X)
            .ok_or_else(|| anyhow!("{} has no ABS_MT_POSITION_X axis", path.display()))?;
        let (min, max) = (info.minimum(), info.maximum());
        device
            .grab()
            .with_context(|| format!("grabbing {}", path.display()))?;
        log::info!("touch device {} (x range {min}..{max})", path.display());
        Ok(Self { device, min, max })
    }

    pub async fn run(self, tx: UnboundedSender<Touch>) -> Result<()> {
        let span = (self.max - self.min).max(1) as f64;
        let min = self.min;
        let mut stream = self.device.into_event_stream()?;
        let mut slots: HashMap<i32, Slot> = HashMap::new();
        let mut current = 0;
        loop {
            let ev = stream.next_event().await?;
            if ev.event_type() == EventType::ABSOLUTE {
                let code = AbsoluteAxisCode(ev.code());
                if code == AbsoluteAxisCode::ABS_MT_SLOT {
                    current = ev.value();
                } else if code == AbsoluteAxisCode::ABS_MT_TRACKING_ID {
                    slots.entry(current).or_default().pending = Some(ev.value() >= 0);
                } else if code == AbsoluteAxisCode::ABS_MT_POSITION_X {
                    slots.entry(current).or_default().x = Some(ev.value());
                }
            } else if ev.event_type() == EventType::SYNCHRONIZATION {
                for (&slot, state) in slots.iter_mut() {
                    let touch = match state.pending.take() {
                        Some(true) => match state.x {
                            Some(x) => Touch::Down {
                                slot,
                                x: (x - min) as f64 / span,
                            },
                            None => continue,
                        },
                        Some(false) => {
                            state.x = None;
                            Touch::Up { slot }
                        }
                        None => continue,
                    };
                    if tx.send(touch).is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}
