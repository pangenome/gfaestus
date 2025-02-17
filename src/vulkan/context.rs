use std::ffi::c_void;

use ash::{Device, Entry, Instance};

use ash::extensions::{
    ext::DebugUtils,
    khr::{PushDescriptor, Surface},
};

use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};

use ash::vk::{KhrGetPhysicalDeviceProperties2Fn, StructureType};

use ash::vk;

pub struct VkContext {
    _entry: Entry,
    instance: Instance,

    debug_utils: Option<(DebugUtils, vk::DebugUtilsMessengerEXT)>,

    surface: Surface,
    surface_khr: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    device: Device,

    push_descriptor: PushDescriptor,

    get_physical_device_features2: KhrGetPhysicalDeviceProperties2Fn,

    pub portability_subset: bool,
}

impl VkContext {
    pub fn push_descriptor(&self) -> &PushDescriptor {
        &self.push_descriptor
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn surface_khr(&self) -> vk::SurfaceKHR {
        self.surface_khr
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.physical_device
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn debug_utils(&self) -> Option<&DebugUtils> {
        self.debug_utils.as_ref().map(|(utils, _)| utils)
    }

    pub fn portability_features(
        &self,
    ) -> anyhow::Result<PortabilitySubsetFeatures> {
        let mut features_2 = vk::PhysicalDeviceFeatures2::builder()
            .features(vk::PhysicalDeviceFeatures::default());

        let mut subset_features = PortabilitySubsetFeaturesKhr::default();
        let subset_ptr: *mut _ = &mut subset_features;
        let subset_ptr = subset_ptr as *mut c_void;
        features_2.p_next = subset_ptr;

        let mut features_2 = features_2.build();

        let features_ptr: *mut vk::PhysicalDeviceFeatures2 = &mut features_2;

        unsafe {
            self.get_physical_device_features2
                .get_physical_device_features2_khr(
                    self.physical_device,
                    features_ptr,
                );
        }

        let subset_features = {
            unsafe {
                let subset: *mut PortabilitySubsetFeaturesKhr =
                    std::mem::transmute(subset_ptr);
                *subset
            }
        };

        Ok(subset_features.features)
    }

    pub fn testin(&self) -> anyhow::Result<()> {
        let mut features_2 = vk::PhysicalDeviceFeatures2::builder()
            .features(vk::PhysicalDeviceFeatures::default());

        let mut atomic_features = ShaderAtomicFloatFeaturesEXT_::default();
        let atomic_ptr: *mut _ = &mut atomic_features;
        let atomic_ptr = atomic_ptr as *mut c_void;
        features_2.p_next = atomic_ptr;

        let mut features_2 = features_2.build();

        let features_ptr: *mut vk::PhysicalDeviceFeatures2 = &mut features_2;

        unsafe {
            self.get_physical_device_features2
                .get_physical_device_features2_khr(
                    self.physical_device,
                    features_ptr,
                );
        }

        let atomic_features = {
            unsafe {
                let atomic: *mut ShaderAtomicFloatFeaturesEXT_ =
                    std::mem::transmute(atomic_ptr);
                *atomic
            }
        };

        log::warn!(
            "shader atomic float features: {:?}",
            atomic_features.features
        );

        Ok(())
    }
}

impl VkContext {
    pub fn new(
        entry: Entry,
        instance: Instance,
        debug_utils: Option<(DebugUtils, vk::DebugUtilsMessengerEXT)>,
        surface: Surface,
        surface_khr: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
        device: Device,
    ) -> Self {
        let push_descriptor = PushDescriptor::new(&instance, &device);

        let get_physical_device_features2 =
            unsafe {
                KhrGetPhysicalDeviceProperties2Fn::load(|name| {
                    std::mem::transmute(entry.get_instance_proc_addr(
                        instance.handle(),
                        name.as_ptr(),
                    ))
                })
            };

        let portability_subset = {
            let extension_props = unsafe {
                instance
                    .enumerate_device_extension_properties(physical_device)
                    .unwrap()
            };

            let portability =
                std::ffi::CString::new("VK_KHR_portability_subset").unwrap();

            extension_props.iter().any(|ext| {
                let name = unsafe {
                    std::ffi::CStr::from_ptr(ext.extension_name.as_ptr())
                };
                portability.as_ref() == name
            })
        };

        // log::warn!("vk_context portability subset: {}", portability_subset);

        VkContext {
            _entry: entry,
            instance,
            debug_utils,
            surface,
            surface_khr,
            physical_device,
            device,

            push_descriptor,
            get_physical_device_features2,
            portability_subset,
        }
    }
}

impl VkContext {
    pub fn get_mem_properties(&self) -> vk::PhysicalDeviceMemoryProperties {
        unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        }
    }

    /// Find the first compatible format from `candidates`.
    pub fn find_supported_format(
        &self,
        candidates: &[vk::Format],
        tiling: vk::ImageTiling,
        features: vk::FormatFeatureFlags,
    ) -> Option<vk::Format> {
        candidates.iter().cloned().find(|candidate| {
            let props = unsafe {
                self.instance.get_physical_device_format_properties(
                    self.physical_device,
                    *candidate,
                )
            };
            (tiling == vk::ImageTiling::LINEAR
                && props.linear_tiling_features.contains(features))
                || (tiling == vk::ImageTiling::OPTIMAL
                    && props.optimal_tiling_features.contains(features))
        })
    }

    /// Return the maximim sample count supported.
    pub fn get_max_usable_sample_count(&self) -> vk::SampleCountFlags {
        let props = unsafe {
            self.instance
                .get_physical_device_properties(self.physical_device)
        };
        let color_sample_counts = props.limits.framebuffer_color_sample_counts;
        let depth_sample_counts = props.limits.framebuffer_depth_sample_counts;
        let sample_counts = color_sample_counts.min(depth_sample_counts);

        if sample_counts.contains(vk::SampleCountFlags::TYPE_64) {
            vk::SampleCountFlags::TYPE_64
        } else if sample_counts.contains(vk::SampleCountFlags::TYPE_32) {
            vk::SampleCountFlags::TYPE_32
        } else if sample_counts.contains(vk::SampleCountFlags::TYPE_16) {
            vk::SampleCountFlags::TYPE_16
        } else if sample_counts.contains(vk::SampleCountFlags::TYPE_8) {
            vk::SampleCountFlags::TYPE_8
        } else if sample_counts.contains(vk::SampleCountFlags::TYPE_4) {
            vk::SampleCountFlags::TYPE_4
        } else if sample_counts.contains(vk::SampleCountFlags::TYPE_2) {
            vk::SampleCountFlags::TYPE_2
        } else {
            vk::SampleCountFlags::TYPE_1
        }
    }
}

impl Drop for VkContext {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_device(None);
            self.surface.destroy_surface(self.surface_khr, None);
            if let Some((report, callback)) = self.debug_utils.take() {
                report.destroy_debug_utils_messenger(callback, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Default, Debug)]
pub struct PortabilitySubsetFeatures {
    pub constant_alpha_color_blend_factors: vk::Bool32,
    pub events: vk::Bool32,
    pub image_view_format_reinterpretation: vk::Bool32,
    pub image_view_format_swizzle: vk::Bool32,
    pub image_view_2d_on_3d_image: vk::Bool32,
    pub multisample_array_image: vk::Bool32,
    pub mutable_comparison_samplers: vk::Bool32,
    pub point_polygons: vk::Bool32,
    pub sampler_mip_lod_bias: vk::Bool32,
    pub separate_stencil_mask_ref: vk::Bool32,
    pub shader_sample_rate_interpolation_functions: vk::Bool32,
    pub tessellation_isolines: vk::Bool32,
    pub tessellation_point_mode: vk::Bool32,
    pub triangle_fans: vk::Bool32,
    pub vertex_attribute_access_beyond_stride: vk::Bool32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct PortabilitySubsetFeaturesKhr {
    pub s_type: StructureType,
    pub p_next: *mut c_void,
    pub features: PortabilitySubsetFeatures,
}

impl std::default::Default for PortabilitySubsetFeaturesKhr {
    fn default() -> Self {
        Self {
            s_type:
                StructureType::PHYSICAL_DEVICE_PORTABILITY_SUBSET_FEATURES_KHR,
            p_next: ::std::ptr::null_mut(),
            features: PortabilitySubsetFeatures::default(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderAtomicFloatFeaturesEXT {
    pub shader_buffer_float_32_atomics: vk::Bool32,
    pub shader_buffer_float_32_atomic_add: vk::Bool32,
    pub shader_buffer_float_64_atomics: vk::Bool32,
    pub shader_buffer_float_64_atomic_add: vk::Bool32,
    pub shader_shared_float_32_atomics: vk::Bool32,
    pub shader_shared_float_32_atomic_ad: vk::Bool32,
    pub shader_shared_float_64_atomics: vk::Bool32,
    pub shader_shared_float_64_atomic_add: vk::Bool32,
    pub shader_image_float_32_atomics: vk::Bool32,
    pub shader_image_float_32_atomic_add: vk::Bool32,
    pub sparse_image_float_32_atomics: vk::Bool32,
    pub sparse_image_float_32_atomic_add: vk::Bool32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShaderAtomicFloatFeaturesEXT_ {
    pub s_type: StructureType,
    pub p_next: *mut c_void,
    pub features: ShaderAtomicFloatFeaturesEXT,
}

impl std::default::Default for ShaderAtomicFloatFeaturesEXT_ {
    fn default() -> Self {
        Self {
            s_type:
                StructureType::PHYSICAL_DEVICE_SHADER_ATOMIC_FLOAT_FEATURES_EXT,
            p_next: ::std::ptr::null_mut(),
            features: ShaderAtomicFloatFeaturesEXT::default(),
        }
    }
}
