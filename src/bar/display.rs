use std::fs::{self, File, OpenOptions};
use std::os::fd::{AsFd, BorrowedFd};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use drm::buffer::{Buffer, DrmFourcc};
use drm::control::dumbbuffer::DumbMapping;
use drm::control::{
    AtomicCommitFlags, ClipRect, Device as ControlDevice, Mode, ResourceHandle, atomic, connector,
    crtc, framebuffer, plane, property,
};
use drm::{ClientCapability, Device as DrmDevice};

const DRIVERS: [&str; 2] = ["adp", "appletbdrm"];

struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl DrmDevice for Card {}
impl ControlDevice for Card {}

/// The Touch Bar panel. The framebuffer is portrait (short x long); callers use
/// landscape coordinates (long x short) and the rotation happens here.
pub struct Display {
    card: Card,
    map: DumbMapping<'static>,
    pitch: usize,
    fb: framebuffer::Handle,
    connector: connector::Handle,
    crtc: crtc::Handle,
    plane: plane::Handle,
    mode: Mode,
    short: u32,
    long: u32,
    flip: bool,
}

fn find_card() -> Result<PathBuf> {
    for entry in fs::read_dir("/sys/class/drm").context("reading /sys/class/drm")? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let is_card = name
            .strip_prefix("card")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_card {
            continue;
        }
        let Ok(driver) = fs::read_link(entry.path().join("device/driver")) else {
            continue;
        };
        if driver
            .file_name()
            .and_then(|d| d.to_str())
            .is_some_and(|d| DRIVERS.contains(&d))
        {
            return Ok(PathBuf::from("/dev/dri").join(name));
        }
    }
    bail!("no Touch Bar display found (no DRM card driven by adp or appletbdrm)")
}

fn find_prop<T: ResourceHandle>(card: &Card, handle: T, name: &str) -> Result<property::Handle> {
    let props = card.get_properties(handle)?;
    for id in props.as_props_and_values().0 {
        if card.get_property(*id)?.name().to_str()? == name {
            return Ok(*id);
        }
    }
    Err(anyhow!("DRM property {name} not found"))
}

impl Display {
    pub fn open(flip: bool) -> Result<Self> {
        let path = find_card()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let card = Card(file);
        card.set_client_capability(ClientCapability::UniversalPlanes, true)?;
        card.set_client_capability(ClientCapability::Atomic, true)?;
        if let Err(e) = card.acquire_master_lock() {
            log::warn!("could not acquire DRM master explicitly: {e}");
        }

        let res = card.resource_handles()?;
        let mut found = None;
        for handle in res.connectors() {
            let info = card.get_connector(*handle, true)?;
            if info.state() == connector::State::Connected {
                found = Some(info);
                break;
            }
        }
        let con = found.ok_or_else(|| anyhow!("Touch Bar connector is not connected"))?;
        let mode = *con
            .modes()
            .first()
            .ok_or_else(|| anyhow!("Touch Bar has no modes"))?;
        let (short, long) = mode.size();
        let (short, long) = (short as u32, long as u32);
        if long / short.max(1) < 30 {
            bail!("{short}x{long} does not look like a Touch Bar");
        }
        let crtc = *res
            .crtcs()
            .first()
            .ok_or_else(|| anyhow!("no CRTC on the Touch Bar card"))?;
        let plane = *card
            .plane_handles()?
            .first()
            .ok_or_else(|| anyhow!("no plane on the Touch Bar card"))?;

        let db =
            card.create_dumb_buffer((short.next_multiple_of(64), long), DrmFourcc::Xrgb8888, 32)?;
        let fb = card.add_framebuffer(&db, 24, 32)?;
        let pitch = db.pitch() as usize;
        // Mapped once for the life of the process.
        let map = card.map_dumb_buffer(Box::leak(Box::new(db)))?;

        let display = Self {
            card,
            map,
            pitch,
            fb,
            connector: con.handle(),
            crtc,
            plane,
            mode,
            short,
            long,
            flip,
        };
        display.modeset()?;
        Ok(display)
    }

    /// Logical size: (length along the bar, height across it).
    pub fn size(&self) -> (u32, u32) {
        (self.long, self.short)
    }

    pub fn modeset(&self) -> Result<()> {
        let card = &self.card;
        let (w, h) = (self.short as u64, self.long as u64);
        let mut req = atomic::AtomicModeReq::new();
        req.add_property(
            self.connector,
            find_prop(card, self.connector, "CRTC_ID")?,
            property::Value::CRTC(Some(self.crtc)),
        );
        let blob = card.create_property_blob(&self.mode)?;
        req.add_property(self.crtc, find_prop(card, self.crtc, "MODE_ID")?, blob);
        req.add_property(
            self.crtc,
            find_prop(card, self.crtc, "ACTIVE")?,
            property::Value::Boolean(true),
        );
        let plane_props = [
            ("FB_ID", property::Value::Framebuffer(Some(self.fb))),
            ("CRTC_ID", property::Value::CRTC(Some(self.crtc))),
            ("SRC_X", property::Value::UnsignedRange(0)),
            ("SRC_Y", property::Value::UnsignedRange(0)),
            ("SRC_W", property::Value::UnsignedRange(w << 16)),
            ("SRC_H", property::Value::UnsignedRange(h << 16)),
            ("CRTC_X", property::Value::SignedRange(0)),
            ("CRTC_Y", property::Value::SignedRange(0)),
            ("CRTC_W", property::Value::UnsignedRange(w)),
            ("CRTC_H", property::Value::UnsignedRange(h)),
        ];
        for (name, value) in plane_props {
            req.add_property(self.plane, find_prop(card, self.plane, name)?, value);
        }
        card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, req)
            .context(
                "modeset on the Touch Bar failed (is tiny-dfr or another program still using it?)",
            )?;
        self.card.dirty_framebuffer(
            self.fb,
            &[ClipRect::new(0, 0, self.short as u16, self.long as u16)],
        )?;
        Ok(())
    }

    /// Logical pixel to framebuffer pixel.
    fn to_fb(&self) -> impl Fn(u32, u32) -> (u32, u32) + use<> {
        let (short, long, flip) = (self.short, self.long, self.flip);
        move |x, y| {
            if flip {
                (y, long - 1 - x)
            } else {
                (short - 1 - y, x)
            }
        }
    }

    /// Write RGB pixels from `pixel` into the logical rectangle at (x0, y0) of size w x h.
    /// Nothing reaches the panel until `flush`.
    pub fn draw(&mut self, x0: u32, y0: u32, w: u32, h: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) {
        let w = w.min(self.long.saturating_sub(x0));
        let h = h.min(self.short.saturating_sub(y0));
        let (short, flip) = (self.short, self.flip);
        for dx in 0..w {
            let column = self.column(x0 + dx);
            for dy in 0..h {
                let y = y0 + dy;
                let fx = if flip { y } else { short - 1 - y } as usize;
                let [r, g, b] = pixel(dx, dy);
                column[fx * 4..][..4].copy_from_slice(&[b, g, r, 0]);
            }
        }
    }

    /// Blacken the full-height logical strip starting at x0 of width w.
    pub fn clear(&mut self, x0: u32, w: u32) {
        for x in x0..(x0 + w).min(self.long) {
            self.column(x).fill(0);
        }
    }

    /// The framebuffer row holding the logical column `x`.
    fn column(&mut self, x: u32) -> &mut [u8] {
        let row = if self.flip { self.long - 1 - x } else { x } as usize;
        &mut self.map.as_mut()[row * self.pitch..][..self.short as usize * 4]
    }

    /// Present the full-height logical strip starting at x0 of width w.
    pub fn flush(&self, x0: u32, w: u32) -> Result<()> {
        let w = w.min(self.long.saturating_sub(x0));
        if w == 0 {
            return Ok(());
        }
        let to_fb = self.to_fb();
        let (a, b) = (to_fb(x0, 0), to_fb(x0 + w - 1, self.short - 1));
        let clip = ClipRect::new(
            a.0.min(b.0) as u16,
            a.1.min(b.1) as u16,
            a.0.max(b.0) as u16 + 1,
            a.1.max(b.1) as u16 + 1,
        );
        self.card.dirty_framebuffer(self.fb, &[clip])?;
        Ok(())
    }
}
