use std::collections::{HashMap, HashSet};

use gpux::{DevicePixels, Size};

use crate::HtmlIslandUploadPath;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HtmlIslandTextureDescriptor {
    pub size: Size<DevicePixels>,
    pub format: wgpu::TextureFormat,
    pub upload_path: HtmlIslandUploadPath,
}

pub struct HtmlIslandTextureEntry {
    descriptor: HtmlIslandTextureDescriptor,
    texture: Option<wgpu::Texture>,
}

pub struct HtmlIslandTextureView {
    pub view: wgpu::TextureView,
    pub size: Size<DevicePixels>,
}

#[derive(Default)]
pub struct HtmlIslandTextureCache {
    entries: HashMap<String, HtmlIslandTextureEntry>,
    reallocation_count: u32,
    pruned_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlIslandTextureCacheStatus {
    Hit,
    Allocated,
    Reallocated,
}

impl HtmlIslandTextureCache {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn reallocation_count(&self) -> u32 {
        self.reallocation_count
    }

    pub fn pruned_count(&self) -> u32 {
        self.pruned_count
    }

    pub fn get_or_create(
        &mut self,
        id: &str,
        descriptor: HtmlIslandTextureDescriptor,
        device: &wgpu::Device,
    ) -> (&wgpu::Texture, HtmlIslandTextureCacheStatus) {
        let status = match self.entries.get(id) {
            Some(entry) if entry.descriptor == descriptor => HtmlIslandTextureCacheStatus::Hit,
            Some(_) => {
                self.reallocation_count = self.reallocation_count.saturating_add(1);
                HtmlIslandTextureCacheStatus::Reallocated
            }
            None => {
                self.reallocation_count = self.reallocation_count.saturating_add(1);
                HtmlIslandTextureCacheStatus::Allocated
            }
        };

        if status != HtmlIslandTextureCacheStatus::Hit {
            self.entries.insert(
                id.to_string(),
                HtmlIslandTextureEntry {
                    descriptor,
                    texture: Some(create_texture(id, descriptor, device)),
                },
            );
        }

        (
            self.entries.get(id).unwrap().texture.as_ref().unwrap(),
            status,
        )
    }

    pub fn texture_view(&self, id: &str) -> Option<HtmlIslandTextureView> {
        let entry = self.entries.get(id)?;
        let texture = entry.texture.as_ref()?;
        Some(HtmlIslandTextureView {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            size: entry.descriptor.size,
        })
    }

    pub fn prune_except<'a>(&mut self, live_ids: impl IntoIterator<Item = &'a str>) -> usize {
        let live_ids = live_ids.into_iter().collect::<HashSet<_>>();
        let old_len = self.entries.len();
        self.entries.retain(|id, _| live_ids.contains(id.as_str()));
        let pruned = old_len.saturating_sub(self.entries.len());
        self.pruned_count = self.pruned_count.saturating_add(pruned as u32);
        pruned
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.entries.remove(id).is_some();
        if removed {
            self.pruned_count = self.pruned_count.saturating_add(1);
        }
        removed
    }

    #[cfg(test)]
    fn record_for_testing(
        &mut self,
        id: &str,
        descriptor: HtmlIslandTextureDescriptor,
    ) -> HtmlIslandTextureCacheStatus {
        let status = match self.entries.get(id) {
            Some(entry) if entry.descriptor == descriptor => HtmlIslandTextureCacheStatus::Hit,
            Some(_) => HtmlIslandTextureCacheStatus::Reallocated,
            None => HtmlIslandTextureCacheStatus::Allocated,
        };

        if status != HtmlIslandTextureCacheStatus::Hit {
            self.reallocation_count = self.reallocation_count.saturating_add(1);
            self.entries.insert(
                id.to_string(),
                HtmlIslandTextureEntry {
                    descriptor,
                    texture: None,
                },
            );
        }

        status
    }
}

fn create_texture(
    id: &str,
    descriptor: HtmlIslandTextureDescriptor,
    device: &wgpu::Device,
) -> wgpu::Texture {
    let width = descriptor.size.width.0.max(1) as u32;
    let height = descriptor.size.height.0.max(1) as u32;

    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(&format!("html_island_texture:{id}")),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: descriptor.format,
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(width: i32, height: i32) -> HtmlIslandTextureDescriptor {
        HtmlIslandTextureDescriptor {
            size: Size {
                width: DevicePixels(width),
                height: DevicePixels(height),
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            upload_path: HtmlIslandUploadPath::CopyElementImageToTexture,
        }
    }

    #[test]
    fn cache_reuses_texture_for_same_id_size_format_and_upload_path() {
        let mut cache = HtmlIslandTextureCache::default();

        assert_eq!(
            cache.record_for_testing("settings", descriptor(64, 32)),
            HtmlIslandTextureCacheStatus::Allocated
        );
        assert_eq!(
            cache.record_for_testing("settings", descriptor(64, 32)),
            HtmlIslandTextureCacheStatus::Hit
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.reallocation_count(), 1);
    }

    #[test]
    fn cache_reallocates_when_physical_size_changes() {
        let mut cache = HtmlIslandTextureCache::default();

        cache.record_for_testing("settings", descriptor(64, 32));

        assert_eq!(
            cache.record_for_testing("settings", descriptor(128, 32)),
            HtmlIslandTextureCacheStatus::Reallocated
        );
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.reallocation_count(), 2);
    }

    #[test]
    fn cache_reallocates_when_surface_format_changes() {
        let mut cache = HtmlIslandTextureCache::default();
        let mut changed = descriptor(64, 32);
        changed.format = wgpu::TextureFormat::Bgra8Unorm;

        cache.record_for_testing("settings", descriptor(64, 32));

        assert_eq!(
            cache.record_for_testing("settings", changed),
            HtmlIslandTextureCacheStatus::Reallocated
        );
        assert_eq!(cache.reallocation_count(), 2);
    }

    #[test]
    fn cache_prunes_entries_absent_from_live_set() {
        let mut cache = HtmlIslandTextureCache::default();
        cache.record_for_testing("settings", descriptor(64, 32));
        cache.record_for_testing("preview", descriptor(16, 16));

        assert_eq!(cache.prune_except(["preview"].into_iter()), 1);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.pruned_count(), 1);
        assert_eq!(
            cache.record_for_testing("preview", descriptor(16, 16)),
            HtmlIslandTextureCacheStatus::Hit
        );
    }

    #[test]
    fn cache_removes_failed_upload_entry_by_id() {
        let mut cache = HtmlIslandTextureCache::default();
        cache.record_for_testing("failed", descriptor(64, 32));
        cache.record_for_testing("healthy", descriptor(16, 16));

        assert!(cache.remove("failed"));

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.pruned_count(), 1);
        assert_eq!(
            cache.record_for_testing("healthy", descriptor(16, 16)),
            HtmlIslandTextureCacheStatus::Hit
        );
    }
}
