use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

const NAMES: [&str; 3] = ["228600000.dsi.0", "display-pipe", "appletb_backlight"];

/// The main screen's backlight, which the Touch Bar's level follows.
pub struct Screen {
    path: PathBuf,
    max: u32,
}

impl Screen {
    pub fn open() -> Result<Self> {
        let dir = fs::read_dir("/sys/class/backlight")?
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                !NAMES.iter().any(|n| name.contains(n))
            })
            .ok_or_else(|| anyhow!("no screen backlight found"))?
            .path();
        let max: u32 = fs::read_to_string(dir.join("max_brightness"))?
            .trim()
            .parse()?;
        if max == 0 {
            return Err(anyhow!("{} has no brightness range", dir.display()));
        }
        log::info!("brightness follows {}", dir.display());
        Ok(Self {
            path: dir.join("brightness"),
            max,
        })
    }

    pub fn fraction(&self) -> Result<f32> {
        let value: u32 = fs::read_to_string(&self.path)?.trim().parse()?;
        Ok(value.min(self.max) as f32 / self.max as f32)
    }
}

pub struct Backlight {
    file: File,
    max: u32,
}

impl Backlight {
    pub fn open() -> Result<Self> {
        let dir = fs::read_dir("/sys/class/backlight")?
            .filter_map(|e| e.ok())
            .find(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                NAMES.iter().any(|n| name.contains(n))
            })
            .ok_or_else(|| anyhow!("no Touch Bar backlight found"))?
            .path();
        let max = fs::read_to_string(dir.join("max_brightness"))?
            .trim()
            .parse()?;
        let path = dir.join("brightness");
        let file = OpenOptions::new()
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {} for writing", path.display()))?;
        Ok(Self { file, max })
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn set(&mut self, value: u32) -> Result<()> {
        self.file
            .write_all(format!("{}\n", value.min(self.max)).as_bytes())?;
        Ok(())
    }
}
