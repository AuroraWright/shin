//! Older temporal owners. Native temporal CPU evidence
//! covers the exact swaps, keyed removal, countdown/update/retirement order,
//! fast-forward removal and WIPE's separate retired-child copying operation.
use anyhow::{Result, anyhow};
use glam::{Mat4, Vec2, Vec4, vec3};
use shin_core::primitives::color::FloatColor4;
use shin_render::{
    ColorBlendType, DrawPrimitive, LayerFragmentShader, RenderProgramWithArguments,
    RenderRequestBuilder,
    render_pass::RenderPass,
    shaders::types::{buffer::VertexSource, texture::TextureSource, vertices::PosTexVertex},
};

use crate::layer::{
    higurashi_properties::HigurashiProperties,
    render_params::{
        higurashi::{HigurashiTransform, native_rotation_z},
        multiply_transforms,
    },
};

/// Screen1498/14a0. Intentionally has no Clone implementation: actual Screen
/// virtual38 b9054 returnsNULL. Construction includes a separate depth owner.
pub(crate) struct Afterimages<T> {
    extent: [u32; 2],
    // Original Screen destructor releases previous before current.
    previous: Option<T>,
    current: T,
}
impl<T> Afterimages<T> {
    pub(crate) fn new(extent: [u32; 2], current: T) -> Self {
        Self {
            extent,
            current,
            previous: None,
        }
    }
    pub(crate) fn current(&self) -> &T {
        &self.current
    }
    /// The same images, re-created at `extent` for a resized desktop window.
    pub(crate) fn resized<U>(
        &self,
        extent: [u32; 2],
        mut image: impl FnMut(&T) -> U,
    ) -> Afterimages<U> {
        Afterimages {
            extent,
            previous: self.previous.as_ref().map(&mut image),
            current: image(&self.current),
        }
    }
    pub(crate) fn previous(&self) -> Option<&T> {
        self.previous.as_ref()
    }
    pub(crate) fn current_and_previous(&mut self) -> (&mut T, Option<&T>) {
        (&mut self.current, self.previous.as_ref())
    }
    /// Call at ba2a8's end, after root/three group updates and wiper retirement.
    /// Execute for every enabled update, even delta0; rendering does not call it.
    pub(crate) fn after_update(
        &mut self,
        sampled39: f32,
        mut allocate: impl FnMut([u32; 2]) -> Result<T>,
    ) -> Result<()> {
        if sampled39 > 0.0 {
            if self.previous.is_none() {
                // Original does not initially black-clear this new color.
                self.previous = Some(allocate(self.extent)?);
            }
            std::mem::swap(&mut self.current, self.previous.as_mut().unwrap());
        } else {
            // Native B.LE includes unordered; NaN releases the previous owner.
            self.previous = None;
        }
        Ok(())
    }
    pub(crate) fn packet(&self, properties: &HigurashiProperties) -> Option<AfterimagePacket> {
        self.previous.as_ref()?;
        AfterimagePacket::new(properties)
    }
}

pub(crate) struct AfterimagePacket {
    pub(crate) projection: Mat4,
    pub(crate) color: Vec4,
    pub(crate) vertices: [PosTexVertex; 4],
}
impl AfterimagePacket {
    pub(crate) fn new(properties: &HigurashiProperties) -> Option<Self> {
        let p = properties.values();
        let integer39 = p[39] as i32; // Native FCVTZS, including saturation/NaN0.
        if integer39 < 1 {
            return None;
        }
        // Actual694f4 uses only ROOT40..43, independently of the general leaf
        // transform/oscillators.74cac multiplies the incoming matrix on the left.
        let pivot = vec3(p[42], p[43], 0.0);
        let mut matrix = multiply_transforms(Mat4::from_translation(-pivot), Mat4::IDENTITY);
        let scale = p[40] * 0.001;
        matrix = multiply_transforms(Mat4::from_scale(vec3(scale, scale, 1.0)), matrix);
        matrix = multiply_transforms(
            native_rotation_z((p[41] * 0.001) * std::f32::consts::TAU),
            matrix,
        );
        matrix = multiply_transforms(Mat4::from_translation(pivot), matrix);
        // Actual74d34(-640,+640,+360,-360,-1,+1), preserving signedzero.
        let centered = Mat4::from_cols_array(&[
            f32::from_bits(0x3acccccd),
            0.0,
            0.0,
            0.0,
            0.0,
            f32::from_bits(0xbb360b61),
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0,
            0.0,
            -0.0,
            0.0,
            -0.0,
            1.0,
        ]);
        matrix = multiply_transforms(centered, matrix);
        matrix = multiply_transforms(matrix, Mat4::from_translation(vec3(-640.0, -360.0, 0.0)));
        Some(Self {
            projection: matrix,
            color: Vec4::splat(integer39 as f32 * 0.001), // Native does not clamp.
            vertices: [
                (0.0, 0.0, 0.0, 0.0),
                (1280.0, 0.0, 1.0, 0.0),
                (0.0, 720.0, 0.0, 1.0),
                (1280.0, 720.0, 1.0, 1.0),
            ]
            .map(|(x, y, u, v)| PosTexVertex {
                position: Vec2::new(x, y),
                texture_position: Vec2::new(u, v),
            }),
        })
    }
    /// Draw into current1498 after group composition and before root6aaec.
    /// This internal pass ignores outer visibility, alpha and stencil priority.
    pub(crate) fn render(&self, pass: &mut RenderPass, previous: TextureSource) {
        pass.run(
            RenderRequestBuilder::new()
                .color_blend_type(ColorBlendType::LayerPremultiplied1)
                .build(
                    RenderProgramWithArguments::HigurashiPostprocess {
                        vertices: VertexSource::VertexData {
                            vertices: &self.vertices,
                        },
                        texture: previous,
                        transform: HigurashiTransform::convert_clip_depth(self.projection),
                        color: FloatColor4::from_vec4(self.color),
                        operation: LayerFragmentShader::Default,
                        param: Vec4::ZERO,
                    },
                    DrawPrimitive::TrianglesStrip,
                ),
        );
    }
}

/// Actual group vector entry: physical keyed slot, retiring time, strong owner.
/// Retiring key=-1 does not overwrite the drawable's native ownerID.
pub(crate) struct Entry<T> {
    pub(crate) key: i32,
    pub(crate) remaining: f32,
    /// WIPE6c590 appends a NULL result even when Movie virtual38 returnsNULL.
    /// The following native traversal would dereference it; desktop traversal
    /// reports this exact boundary and does not invent a replacement/expiry.
    pub(crate) owner: Option<T>,
}
#[derive(Default)]
pub(crate) struct Group<T> {
    entries: Vec<Entry<T>>,
}
impl<T> Group<T> {
    pub(crate) fn new(entries: Vec<Entry<T>>) -> Self {
        Self { entries }
    }
    pub(crate) fn entries(&self) -> &[Entry<T>] {
        &self.entries
    }
    pub(crate) fn entries_mut(&mut self) -> &mut [Entry<T>] {
        &mut self.entries
    }
    /// Actual6b944 releases complete entry owners from vector end to begin.
    /// An array-backed group clears its keyed suffix before calling this.
    pub(crate) fn clear_reverse(&mut self) {
        while let Some(entry) = self.entries.pop() {
            drop(entry);
        }
    }
    /// Actual6bb18 publishes by physical slot. Replacement preserves the
    /// entry's second word; insertion initializes that active flags word to1.
    pub(crate) fn publish(&mut self, key: i32, owner: T) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.owner = Some(owner);
        } else {
            let index = self.entries.partition_point(|entry| entry.key < key);
            self.entries.insert(
                index,
                Entry {
                    key,
                    remaining: f32::from_bits(1),
                    owner: Some(owner),
                },
            );
        }
    }

    /// 6c280 searches a sorted physicalkey; it does not search ownerID or the
    /// now-freed VM logical mapping. Missing entries are a native no-op.
    pub(crate) fn unload(&mut self, key: i32, delay: f32) {
        let Some(index) = self.entries.iter().position(|entry| entry.key == key) else {
            return;
        };
        let mut entry = self.entries.remove(index);
        if delay > 0.0 {
            entry.key = -1;
            entry.remaining = delay;
            self.entries.insert(0, entry); // Newest negative-key entry goes first.
        }
    }
    /// An array-backed physical group moves its keyed owner here unchanged.
    /// This list is the negative-key prefix of the same native entry vector.
    pub(crate) fn prepend_retiring(&mut self, owner: T, delay: f32) {
        self.entries.insert(
            0,
            Entry {
                key: -1,
                remaining: delay,
                owner: Some(owner),
            },
        );
    }
    /// 6dbf0 first advances the group's base clocks (caller), then processes
    /// each vector row in order, then retires expired rows after ALL updates.
    pub(crate) fn advance(
        &mut self,
        delta: f32,
        advance: impl FnMut(&mut T, f32) -> Result<()>,
    ) -> Result<()> {
        self.advance_with_tail(delta, 0, advance, || Ok(()))
    }
    /// Physical keyed owners may remain in an array suffix. Update that entire
    /// suffix before compacting, and include it in the native kept length.
    pub(crate) fn advance_with_tail(
        &mut self,
        delta: f32,
        kept_tail: usize,
        mut advance: impl FnMut(&mut T, f32) -> Result<()>,
        advance_tail: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        for entry in &mut self.entries {
            if entry.key < 0 {
                entry.remaining -= delta;
            }
            let owner = entry.owner.as_mut().ok_or_else(|| {
                anyhow!("Native retiring group contains a NULL copy before child update")
            })?;
            advance(owner, delta)?;
        }
        // Native HI retains unordered timers. NaN is not an ordered<=0 expiry.
        advance_tail()?;
        self.retire_matching_with_tail(kept_tail, |entry| entry.key < 0 && entry.remaining <= 0.0);
        Ok(())
    }
    /// Native skip forwards to every child BEFORE removing all retiring rows,
    /// irrespective of remaining time. It never swaps afterimage textures.
    pub(crate) fn fast_forward(&mut self, skip: impl FnMut(&mut T) -> Result<()>) -> Result<()> {
        self.fast_forward_with_tail(0, skip, || Ok(()))
    }
    pub(crate) fn fast_forward_with_tail(
        &mut self,
        kept_tail: usize,
        mut skip: impl FnMut(&mut T) -> Result<()>,
        skip_tail: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        for entry in &mut self.entries {
            skip(entry.owner.as_mut().ok_or_else(|| {
                anyhow!("Native retiring group contains a NULL copy before child skip")
            })?)?;
        }
        skip_tail()?;
        self.retire_matching_with_tail(kept_tail, |entry| entry.key < 0);
        Ok(())
    }
    /// Native compaction assigns kept strong handles into earlier holes, then
    /// destroys the unused tail in reverse. Preserve complete-owner drop order
    /// so Movie/workers retire at the actual callback boundary.
    fn retire_matching_with_tail(&mut self, kept_tail: usize, remove: impl Fn(&Entry<T>) -> bool) {
        // Native compaction retains the existing vector when every row lives.
        // Avoid relocating concrete Rust owners and allocating on every tick.
        if !self.entries.iter().any(&remove) {
            return;
        }
        let keep_count = kept_tail + self.entries.iter().filter(|entry| !remove(entry)).count();
        let old = std::mem::take(&mut self.entries);
        let mut kept = Vec::with_capacity(old.capacity());
        let mut tail_removed = Vec::new();
        for (index, entry) in old.into_iter().enumerate() {
            if remove(&entry) {
                if index < keep_count {
                    drop(entry);
                } else {
                    tail_removed.push(entry);
                }
            } else {
                kept.push(entry);
            }
        }
        while let Some(entry) = tail_removed.pop() {
            drop(entry);
        }
        self.entries = kept;
    }

    /// WIPE6c590's distinct operation, NOT Group/Screen RustClone. Destination
    /// retirees are released first. Source retirees are independently native-
    /// copied, and each is inserted atbegin, reversing their source order.
    pub(crate) fn copy_retired_from(
        &mut self,
        source: &Self,
        copy: impl FnMut(&T) -> Result<Option<T>>,
    ) -> Result<()> {
        self.copy_retired_from_with_tail(source, 0, copy)
    }
    pub(crate) fn copy_retired_from_with_tail(
        &mut self,
        source: &Self,
        kept_tail: usize,
        mut copy: impl FnMut(&T) -> Result<Option<T>>,
    ) -> Result<()> {
        self.retire_matching_with_tail(kept_tail, |entry| entry.key < 0);
        for entry in source.entries.iter().filter(|entry| entry.key < 0) {
            let owner = copy(entry.owner.as_ref().ok_or_else(|| {
                anyhow!("Native source group contains a NULL retiring owner before copy")
            })?)?;
            self.entries.insert(
                0,
                Entry {
                    key: -1,
                    remaining: entry.remaining,
                    owner,
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod adv_cache_integration_tests;
#[cfg(test)]
pub(crate) mod adv_cache_pixels;
#[cfg(test)]
pub(crate) mod afterimage_pixels;
#[cfg(test)]
mod frame_integration_tests;
#[cfg(test)]
pub(crate) mod frame_pixels;
#[cfg(test)]
mod tail_tests;
#[cfg(test)]
mod tests;
