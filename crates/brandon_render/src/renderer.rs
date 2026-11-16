use crate::shader::{compile_wgsl, FULLSCREEN_WGSL, MESH_WGSL, QUAD_WGSL};
use crate::{Filter, Frame, MeshVertex, RenderError};
use ash::{ext, khr, vk};
use brandon_core::ImageData;
use glam::{Mat4, Vec3};
use gpu_allocator::vulkan::{
    Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
};
use gpu_allocator::MemoryLocation;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::any::Any;
use std::ffi::{c_char, c_void, CStr};
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const FRAMES_IN_FLIGHT: usize = 2;
const SETS_PER_POOL: u32 = 256;
const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

static VALIDATION_ERRORS: AtomicU32 = AtomicU32::new(0);

/// The texture index that stands for the blurred backdrop.
const BACKDROP_INDEX: u32 = u32::MAX - 1;
/// How many times the backdrop is halved in size while blurring. Each step
/// roughly doubles how far the blur reaches.
const BLUR_LEVELS: usize = 4;
/// How much more vivid the backdrop's colours are than the scene's.
const BACKDROP_SATURATION: f32 = 1.5;
/// Frames with a user interface draw their layers into an image of this
/// format first. Every Vulkan device can draw into it and read it back.
const OFFSCREEN_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
/// The user interface is drawn into an image of this format. It stores
/// sRGB values as they are, so translucent UI colours blend in gamma space,
/// which is what designers and UI toolkits expect.
const UI_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// A texture on the graphics card. Cheap to copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Texture {
    index: u32,
    width: u32,
    height: u32,
}

impl Texture {
    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub fn size(self) -> glam::Vec2 {
        glam::Vec2::new(self.width as f32, self.height as f32)
    }

    /// A handle that refers to no real texture, for tests that only need a size.
    #[doc(hidden)]
    pub fn placeholder(width: u32, height: u32) -> Texture {
        Texture {
            index: u32::MAX,
            width,
            height,
        }
    }
}

struct GpuTexture {
    image: vk::Image,
    view: vk::ImageView,
    allocation: Allocation,
    set: vk::DescriptorSet,
    width: u32,
    height: u32,
    /// Is the data given to this texture already premultiplied by alpha?
    premultiplied: bool,
}

struct GpuBuffer {
    buffer: vk::Buffer,
    allocation: Allocation,
}

struct Swapchain {
    handle: vk::SwapchainKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    /// One per image, signalled when rendering to that image is done.
    rendered: Vec<vk::Semaphore>,
    can_capture: bool,
}

struct FrameSlot {
    cmd: vk::CommandBuffer,
    fence: vk::Fence,
    image_available: vk::Semaphore,
    instances: Option<GpuBuffer>,
    capacity: usize,
    mesh_vertices: GrowBuffer,
    mesh_indices: GrowBuffer,
}

/// A buffer that is replaced by a bigger one when it gets too small.
#[derive(Default)]
struct GrowBuffer {
    buffer: Option<GpuBuffer>,
    /// Size in bytes.
    capacity: usize,
}

/// An image the renderer draws into and then reads from.
struct Target {
    image: vk::Image,
    view: vk::ImageView,
    allocation: Allocation,
    set: vk::DescriptorSet,
    width: u32,
    height: u32,
}

impl Target {
    fn extent(&self) -> vk::Extent2D {
        vk::Extent2D {
            width: self.width,
            height: self.height,
        }
    }
}

/// The images used for frames with a user interface.
struct Offscreen {
    /// The layers, drawn at full size.
    scene: Target,
    /// The layers again with the UI on top, in gamma space.
    ui: Target,
    /// Ever smaller copies of the scene, for blurring. After blurring, the
    /// first holds the backdrop.
    levels: Vec<Target>,
    extent: vk::Extent2D,
}

/// Push constants for [`FULLSCREEN_WGSL`].
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FullscreenParams {
    texel: [f32; 2],
    spread: f32,
    mode: u32,
    saturation: f32,
    _padding: f32,
}

impl FullscreenParams {
    fn new(source: Option<&Target>, spread: f32, mode: u32) -> Self {
        let texel = source.map_or([0.0; 2], |t| [1.0 / t.width as f32, 1.0 / t.height as f32]);
        FullscreenParams {
            texel,
            spread,
            mode,
            saturation: 1.0,
            _padding: 0.0,
        }
    }

    fn shrink(source: &Target) -> Self {
        Self::new(Some(source), 1.0, 0)
    }

    fn grow(source: &Target) -> Self {
        Self::new(Some(source), 1.0, 1)
    }

    /// Copies a linear image into a gamma-space one.
    fn encode() -> Self {
        Self::new(None, 0.0, 3)
    }

    /// Copies a gamma-space image into a linear one.
    fn decode() -> Self {
        Self::new(None, 0.0, 4)
    }
}

/// One UI mesh, ready to draw.
struct MeshDraw {
    texture: u32,
    filter: Filter,
    first_index: u32,
    index_count: u32,
    vertex_offset: i32,
    scissor: vk::Rect2D,
}

/// One quad as the shader sees it.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    axes: [f32; 4],
    origin: [f32; 2],
    uv: [f32; 4],
    color: [f32; 4],
}

struct Batch {
    texture: u32,
    filter: Filter,
    first: u32,
    count: u32,
}

struct LayerDraw {
    clip_from_world: Mat4,
    batches: Vec<Batch>,
}

/// Draws [`Frame`]s to a window with Vulkan.
pub struct Renderer {
    entry: ash::Entry,
    instance: ash::Instance,
    debug: Option<(ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    surface_loader: khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    swapchain_loader: khr::swapchain::Device,
    rendering: khr::dynamic_rendering::Device,
    allocator: ManuallyDrop<Allocator>,
    swapchain: Swapchain,
    pool: vk::CommandPool,
    frames: Vec<FrameSlot>,
    frame_index: usize,
    sampler_layout: vk::DescriptorSetLayout,
    texture_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// Draws quads into the offscreen scene image.
    offscreen_pipeline: vk::Pipeline,
    /// Draws UI meshes into the offscreen UI image.
    mesh_pipeline: vk::Pipeline,
    fullscreen_layout: vk::PipelineLayout,
    /// Full-window passes into offscreen images, the UI image and the window.
    blur_pipeline: vk::Pipeline,
    compose_pipeline: vk::Pipeline,
    present_pipeline: vk::Pipeline,
    offscreen: Option<Offscreen>,
    /// Descriptor sets for the offscreen images, kept across resizes.
    offscreen_sets: Vec<vk::DescriptorSet>,
    samplers: [vk::Sampler; 2],
    sampler_sets: [vk::DescriptorSet; 2],
    descriptor_pools: Vec<vk::DescriptorPool>,
    sets_left: u32,
    textures: Vec<Option<GpuTexture>>,
    white: Texture,
    wanted_size: (u32, u32),
    needs_recreate: bool,
    capture_requested: bool,
    captured: Option<ImageData>,
    device_name: String,
    // Kept last so the window outlives everything that draws to it.
    _window: Arc<dyn Any>,
}

fn vk_err(doing: &'static str) -> impl Fn(vk::Result) -> RenderError {
    move |result| RenderError::Vulkan { doing, result }
}

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut c_void,
) -> vk::Bool32 {
    let message = if data.is_null() || (*data).p_message.is_null() {
        std::borrow::Cow::Borrowed("(no message)")
    } else {
        CStr::from_ptr((*data).p_message).to_string_lossy()
    };
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        VALIDATION_ERRORS.fetch_add(1, Ordering::Relaxed);
        log::error!("Vulkan validation: {message}");
    } else {
        log::warn!("Vulkan validation: {message}");
    }
    vk::FALSE
}

impl Renderer {
    /// Creates a renderer drawing to `window`, whose drawable area is
    /// `width` by `height` physical pixels.
    ///
    /// Set the environment variable `BRANDON_VALIDATION=1` to turn on the
    /// Vulkan validation layers, if they are installed.
    pub fn new<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self, RenderError>
    where
        W: HasDisplayHandle + HasWindowHandle + 'static,
    {
        let display = window
            .display_handle()
            .map_err(|e| RenderError::Window(e.to_string()))?
            .as_raw();
        let window_handle = window
            .window_handle()
            .map_err(|e| RenderError::Window(e.to_string()))?
            .as_raw();

        unsafe {
            let entry = ash::Entry::load().map_err(|e| RenderError::NoVulkan(e.to_string()))?;

            // --- Instance ---------------------------------------------------
            let mut extensions: Vec<*const c_char> =
                ash_window::enumerate_required_extensions(display)
                    .map_err(vk_err("listing window extensions"))?
                    .to_vec();
            let available_extensions = entry
                .enumerate_instance_extension_properties(None)
                .map_err(vk_err("listing instance extensions"))?;
            let has_instance_ext = |name: &CStr| {
                available_extensions
                    .iter()
                    .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == name))
            };
            let mut flags = vk::InstanceCreateFlags::empty();
            if has_instance_ext(khr::portability_enumeration::NAME) {
                // Needed to see MoltenVK on macOS.
                extensions.push(khr::portability_enumeration::NAME.as_ptr());
                flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
            }

            let want_validation =
                std::env::var("BRANDON_VALIDATION").is_ok_and(|v| v != "0" && !v.is_empty());
            let mut layers: Vec<*const c_char> = Vec::new();
            let mut use_debug = false;
            if want_validation {
                let available = entry
                    .enumerate_instance_layer_properties()
                    .unwrap_or_default();
                let has_layer = available
                    .iter()
                    .any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == VALIDATION_LAYER));
                if has_layer && has_instance_ext(ext::debug_utils::NAME) {
                    layers.push(VALIDATION_LAYER.as_ptr());
                    extensions.push(ext::debug_utils::NAME.as_ptr());
                    use_debug = true;
                } else {
                    log::warn!("BRANDON_VALIDATION is set, but the Vulkan validation layers are not installed");
                }
            }

            let app_info = vk::ApplicationInfo::default()
                .application_name(c"Brandon game")
                .engine_name(c"Brandon")
                .api_version(vk::API_VERSION_1_2);
            let instance_info = vk::InstanceCreateInfo::default()
                .application_info(&app_info)
                .enabled_extension_names(&extensions)
                .enabled_layer_names(&layers)
                .flags(flags);
            let instance = entry
                .create_instance(&instance_info, None)
                .map_err(vk_err("creating the Vulkan instance"))?;

            let debug = if use_debug {
                let loader = ext::debug_utils::Instance::new(&entry, &instance);
                let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                    )
                    .message_type(
                        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                            | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    )
                    .pfn_user_callback(Some(debug_callback));
                loader
                    .create_debug_utils_messenger(&info, None)
                    .ok()
                    .map(|messenger| (loader, messenger))
            } else {
                None
            };

            // --- Surface and device -----------------------------------------
            let surface_loader = khr::surface::Instance::new(&entry, &instance);
            let surface =
                ash_window::create_surface(&entry, &instance, display, window_handle, None)
                    .map_err(vk_err("creating the window surface"))?;

            let (physical, queue_family, device_name, needs_portability_subset) =
                pick_device(&instance, &surface_loader, surface)?;
            log::info!("Brandon renderer: using {device_name}");

            let mut device_extensions = vec![
                khr::swapchain::NAME.as_ptr(),
                khr::dynamic_rendering::NAME.as_ptr(),
            ];
            if needs_portability_subset {
                device_extensions.push(khr::portability_subset::NAME.as_ptr());
            }
            let queue_infos = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(queue_family)
                .queue_priorities(&[1.0])];
            let mut dynamic_rendering =
                vk::PhysicalDeviceDynamicRenderingFeatures::default().dynamic_rendering(true);
            let device_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queue_infos)
                .enabled_extension_names(&device_extensions)
                .push_next(&mut dynamic_rendering);
            let device = instance
                .create_device(physical, &device_info, None)
                .map_err(vk_err("creating the Vulkan device"))?;
            let queue = device.get_device_queue(queue_family, 0);
            let swapchain_loader = khr::swapchain::Device::new(&instance, &device);
            let rendering = khr::dynamic_rendering::Device::new(&instance, &device);

            let allocator = Allocator::new(&AllocatorCreateDesc {
                instance: instance.clone(),
                device: device.clone(),
                physical_device: physical,
                debug_settings: Default::default(),
                buffer_device_address: false,
                allocation_sizes: Default::default(),
            })
            .map_err(|e| RenderError::Memory(e.to_string()))?;

            // --- Commands and per-frame data ----------------------------------
            let pool = device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(queue_family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .map_err(vk_err("creating the command pool"))?;
            let cmds = device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(FRAMES_IN_FLIGHT as u32),
                )
                .map_err(vk_err("allocating command buffers"))?;
            let mut frames = Vec::new();
            for cmd in cmds {
                frames.push(FrameSlot {
                    cmd,
                    fence: device
                        .create_fence(
                            &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                            None,
                        )
                        .map_err(vk_err("creating a fence"))?,
                    image_available: device
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                        .map_err(vk_err("creating a semaphore"))?,
                    instances: None,
                    capacity: 0,
                    mesh_vertices: GrowBuffer::default(),
                    mesh_indices: GrowBuffer::default(),
                });
            }

            // --- Descriptor layouts, samplers and the pipeline -------------------
            let sampler_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(0)
                            .descriptor_type(vk::DescriptorType::SAMPLER)
                            .descriptor_count(1)
                            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                    ]),
                    None,
                )
                .map_err(vk_err("creating a descriptor layout"))?;
            let texture_layout = device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(0)
                            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                            .descriptor_count(1)
                            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
                    ]),
                    None,
                )
                .map_err(vk_err("creating a descriptor layout"))?;
            let pipeline_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&[sampler_layout, texture_layout])
                        .push_constant_ranges(&[vk::PushConstantRange::default()
                            .stage_flags(vk::ShaderStageFlags::VERTEX)
                            .offset(0)
                            .size(64)]),
                    None,
                )
                .map_err(vk_err("creating the pipeline layout"))?;

            let fullscreen_layout = device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&[sampler_layout, texture_layout])
                        .push_constant_ranges(&[vk::PushConstantRange::default()
                            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                            .offset(0)
                            .size(std::mem::size_of::<FullscreenParams>() as u32)]),
                    None,
                )
                .map_err(vk_err("creating the pipeline layout"))?;

            let make_sampler = |filter: vk::Filter| {
                device.create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(filter)
                        .min_filter(filter)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
            };
            let samplers = [
                make_sampler(vk::Filter::LINEAR).map_err(vk_err("creating a sampler"))?,
                make_sampler(vk::Filter::NEAREST).map_err(vk_err("creating a sampler"))?,
            ];

            let swapchain = create_swapchain(
                &device,
                &surface_loader,
                &swapchain_loader,
                physical,
                surface,
                (width, height),
                vk::SwapchainKHR::null(),
            )?;
            let pipeline = create_quad_pipeline(&device, pipeline_layout, swapchain.format)?;
            let offscreen_pipeline =
                create_quad_pipeline(&device, pipeline_layout, OFFSCREEN_FORMAT)?;
            let mesh_pipeline = create_mesh_pipeline(&device, pipeline_layout, UI_FORMAT)?;
            let blur_pipeline =
                create_fullscreen_pipeline(&device, fullscreen_layout, OFFSCREEN_FORMAT)?;
            let compose_pipeline =
                create_fullscreen_pipeline(&device, fullscreen_layout, UI_FORMAT)?;
            let present_pipeline =
                create_fullscreen_pipeline(&device, fullscreen_layout, swapchain.format)?;

            let mut renderer = Renderer {
                entry,
                instance,
                debug,
                surface_loader,
                surface,
                physical,
                device,
                queue,
                swapchain_loader,
                rendering,
                allocator: ManuallyDrop::new(allocator),
                swapchain,
                pool,
                frames,
                frame_index: 0,
                sampler_layout,
                texture_layout,
                pipeline_layout,
                pipeline,
                offscreen_pipeline,
                mesh_pipeline,
                fullscreen_layout,
                blur_pipeline,
                compose_pipeline,
                present_pipeline,
                offscreen: None,
                offscreen_sets: Vec::new(),
                samplers,
                sampler_sets: [vk::DescriptorSet::null(); 2],
                descriptor_pools: Vec::new(),
                sets_left: 0,
                textures: Vec::new(),
                white: Texture {
                    index: 0,
                    width: 1,
                    height: 1,
                },
                wanted_size: (width, height),
                needs_recreate: false,
                capture_requested: false,
                captured: None,
                device_name,
                _window: window,
            };

            for i in 0..2 {
                let set = renderer.allocate_set(renderer.sampler_layout)?;
                let info = [vk::DescriptorImageInfo::default().sampler(renderer.samplers[i])];
                renderer.device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::SAMPLER)
                        .image_info(&info)],
                    &[],
                );
                renderer.sampler_sets[i] = set;
            }
            renderer.white = renderer.create_texture(1, 1, &[255, 255, 255, 255])?;
            Ok(renderer)
        }
    }

    /// The name of the graphics device in use.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// A 1x1 white texture, for drawing plain colours.
    pub fn white(&self) -> Texture {
        self.white
    }

    /// The size of the drawable area in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.swapchain.extent.width, self.swapchain.extent.height)
    }

    /// How many errors the Vulkan validation layers have reported. Always
    /// zero unless validation is on (see [`Renderer::new`]).
    pub fn validation_errors() -> u32 {
        VALIDATION_ERRORS.load(Ordering::Relaxed)
    }

    /// Tells the renderer the window's drawable area changed size.
    pub fn resize(&mut self, width: u32, height: u32) {
        if (width, height) != self.wanted_size {
            self.wanted_size = (width, height);
            self.needs_recreate = true;
        }
    }

    // --- Textures ----------------------------------------------------------

    /// Uploads an image. `rgba` is 8-bit sRGB with straight alpha, rows from
    /// top to bottom.
    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<Texture, RenderError> {
        self.create_texture_inner(width, height, rgba, false)
    }

    /// Uploads an image whose colours are already multiplied by alpha, as UI
    /// toolkits provide them. Updates to this texture must be premultiplied
    /// too.
    pub fn create_premultiplied_texture(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<Texture, RenderError> {
        self.create_texture_inner(width, height, rgba, true)
    }

    fn create_texture_inner(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
        premultiplied: bool,
    ) -> Result<Texture, RenderError> {
        assert_eq!(
            rgba.len(),
            (width * height * 4) as usize,
            "texture data does not match its size"
        );
        assert!(width > 0 && height > 0, "textures cannot be empty");
        unsafe {
            let image = self
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_SRGB)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .map_err(vk_err("creating a texture"))?;
            let allocation = self
                .allocator
                .allocate(&AllocationCreateDesc {
                    name: "texture",
                    requirements: self.device.get_image_memory_requirements(image),
                    location: MemoryLocation::GpuOnly,
                    linear: false,
                    allocation_scheme: AllocationScheme::GpuAllocatorManaged,
                })
                .map_err(|e| RenderError::Memory(e.to_string()))?;
            self.device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
                .map_err(vk_err("binding texture memory"))?;
            let view = self
                .device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_SRGB)
                        .subresource_range(color_range()),
                    None,
                )
                .map_err(vk_err("creating a texture view"))?;
            let set = self.allocate_set(self.texture_layout)?;
            let info = [vk::DescriptorImageInfo::default()
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            self.device.update_descriptor_sets(
                &[vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                    .image_info(&info)],
                &[],
            );

            let texture = GpuTexture {
                image,
                view,
                allocation,
                set,
                width,
                height,
                premultiplied,
            };
            self.upload(&texture, rgba, vk::ImageLayout::UNDEFINED)?;
            let index = match self.textures.iter().position(Option::is_none) {
                Some(free) => {
                    self.textures[free] = Some(texture);
                    free
                }
                None => {
                    self.textures.push(Some(texture));
                    self.textures.len() - 1
                }
            };
            Ok(Texture {
                index: index as u32,
                width,
                height,
            })
        }
    }

    /// Replaces the whole contents of a texture. The data must be the same
    /// size as the texture.
    pub fn update_texture(&mut self, texture: Texture, rgba: &[u8]) -> Result<(), RenderError> {
        assert_eq!(
            rgba.len(),
            (texture.width * texture.height * 4) as usize,
            "texture data does not match its size"
        );
        unsafe {
            // A frame that is still being drawn may be using the texture.
            self.device
                .device_wait_idle()
                .map_err(vk_err("waiting for the device"))?;
            let Some(gpu) = self
                .textures
                .get_mut(texture.index as usize)
                .and_then(Option::take)
            else {
                return Ok(());
            };
            let result = self.upload(&gpu, rgba, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
            self.textures[texture.index as usize] = Some(gpu);
            result
        }
    }

    /// Frees a texture. Quads that still use it draw nothing.
    pub fn destroy_texture(&mut self, texture: Texture) {
        if texture == self.white {
            return;
        }
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(gpu) = self
                .textures
                .get_mut(texture.index as usize)
                .and_then(Option::take)
            {
                self.free_texture(gpu);
            }
        }
    }

    unsafe fn free_texture(&mut self, gpu: GpuTexture) {
        self.device.destroy_image_view(gpu.view, None);
        self.device.destroy_image(gpu.image, None);
        let _ = self.allocator.free(gpu.allocation);
        // The descriptor set goes back when its pool is destroyed.
    }

    unsafe fn upload(
        &mut self,
        texture: &GpuTexture,
        rgba: &[u8],
        old_layout: vk::ImageLayout,
    ) -> Result<(), RenderError> {
        let mut staging = self.create_buffer(
            rgba.len() as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            "texture upload",
        )?;
        let mapped = staging
            .allocation
            .mapped_slice_mut()
            .expect("upload buffers are mapped");
        if texture.premultiplied {
            mapped[..rgba.len()].copy_from_slice(rgba);
        } else {
            premultiply_into(rgba, &mut mapped[..rgba.len()]);
        }

        let cmd = self
            .device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .map_err(vk_err("allocating a command buffer"))?[0];
        let record_and_submit = || -> Result<(), vk::Result> {
            self.device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let (src_access, src_stage) = if old_layout == vk::ImageLayout::UNDEFINED {
                (
                    vk::AccessFlags::empty(),
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                )
            } else {
                (
                    vk::AccessFlags::SHADER_READ,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                )
            };
            self.barrier(
                cmd,
                texture.image,
                (old_layout, vk::ImageLayout::TRANSFER_DST_OPTIMAL),
                (src_access, vk::AccessFlags::TRANSFER_WRITE),
                (src_stage, vk::PipelineStageFlags::TRANSFER),
            );
            self.device.cmd_copy_buffer_to_image(
                cmd,
                staging.buffer,
                texture.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .image_subresource(color_layers())
                    .image_extent(vk::Extent3D {
                        width: texture.width,
                        height: texture.height,
                        depth: 1,
                    })],
            );
            self.barrier(
                cmd,
                texture.image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                ),
                (
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::SHADER_READ,
                ),
                (
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                ),
            );
            self.device.end_command_buffer(cmd)?;
            self.device.queue_submit(
                self.queue,
                &[vk::SubmitInfo::default().command_buffers(&[cmd])],
                vk::Fence::null(),
            )?;
            self.device.queue_wait_idle(self.queue)
        };
        let result = record_and_submit().map_err(vk_err("uploading a texture"));
        self.device.free_command_buffers(self.pool, &[cmd]);
        self.destroy_buffer(staging);
        result
    }

    unsafe fn barrier(
        &self,
        cmd: vk::CommandBuffer,
        image: vk::Image,
        layouts: (vk::ImageLayout, vk::ImageLayout),
        access: (vk::AccessFlags, vk::AccessFlags),
        stages: (vk::PipelineStageFlags, vk::PipelineStageFlags),
    ) {
        self.device.cmd_pipeline_barrier(
            cmd,
            stages.0,
            stages.1,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[vk::ImageMemoryBarrier::default()
                .old_layout(layouts.0)
                .new_layout(layouts.1)
                .src_access_mask(access.0)
                .dst_access_mask(access.1)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(color_range())],
        );
    }

    unsafe fn create_buffer(
        &mut self,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
        name: &str,
    ) -> Result<GpuBuffer, RenderError> {
        let buffer = self
            .device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size.max(1))
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
            .map_err(vk_err("creating a buffer"))?;
        let allocation = self
            .allocator
            .allocate(&AllocationCreateDesc {
                name,
                requirements: self.device.get_buffer_memory_requirements(buffer),
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| RenderError::Memory(e.to_string()))?;
        self.device
            .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
            .map_err(vk_err("binding buffer memory"))?;
        Ok(GpuBuffer { buffer, allocation })
    }

    unsafe fn destroy_buffer(&mut self, buffer: GpuBuffer) {
        self.device.destroy_buffer(buffer.buffer, None);
        let _ = self.allocator.free(buffer.allocation);
    }

    unsafe fn allocate_set(
        &mut self,
        layout: vk::DescriptorSetLayout,
    ) -> Result<vk::DescriptorSet, RenderError> {
        if self.sets_left == 0 {
            let sizes = [
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLED_IMAGE)
                    .descriptor_count(SETS_PER_POOL),
                vk::DescriptorPoolSize::default()
                    .ty(vk::DescriptorType::SAMPLER)
                    .descriptor_count(SETS_PER_POOL),
            ];
            let pool = self
                .device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .max_sets(SETS_PER_POOL)
                        .pool_sizes(&sizes),
                    None,
                )
                .map_err(vk_err("creating a descriptor pool"))?;
            self.descriptor_pools.push(pool);
            self.sets_left = SETS_PER_POOL;
        }
        let pool = *self
            .descriptor_pools
            .last()
            .expect("a pool was just created");
        let set = self
            .device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&[layout]),
            )
            .map_err(vk_err("allocating a descriptor set"))?[0];
        self.sets_left -= 1;
        Ok(set)
    }

    // --- Drawing -----------------------------------------------------------

    /// Asks for the next rendered frame to be copied out. Collect it with
    /// [`take_capture`](Self::take_capture) after the next [`render`](Self::render).
    pub fn request_capture(&mut self) {
        self.capture_requested = true;
    }

    /// The frame captured after [`request_capture`](Self::request_capture), if it is ready.
    pub fn take_capture(&mut self) -> Option<ImageData> {
        self.captured.take()
    }

    unsafe fn recreate_swapchain(&mut self) -> Result<(), RenderError> {
        self.device
            .device_wait_idle()
            .map_err(vk_err("waiting for the device"))?;
        let old = std::mem::replace(
            &mut self.swapchain,
            Swapchain {
                handle: vk::SwapchainKHR::null(),
                format: vk::Format::UNDEFINED,
                extent: vk::Extent2D::default(),
                images: Vec::new(),
                views: Vec::new(),
                rendered: Vec::new(),
                can_capture: false,
            },
        );
        let old_format = old.format;
        if let Some(offscreen) = self.offscreen.take() {
            self.destroy_offscreen(offscreen);
        }
        for view in &old.views {
            self.device.destroy_image_view(*view, None);
        }
        for semaphore in &old.rendered {
            self.device.destroy_semaphore(*semaphore, None);
        }
        let created = create_swapchain(
            &self.device,
            &self.surface_loader,
            &self.swapchain_loader,
            self.physical,
            self.surface,
            self.wanted_size,
            old.handle,
        );
        self.swapchain_loader.destroy_swapchain(old.handle, None);
        self.swapchain = created?;
        if self.swapchain.format != old_format && self.swapchain.format != vk::Format::UNDEFINED {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline(self.present_pipeline, None);
            self.pipeline =
                create_quad_pipeline(&self.device, self.pipeline_layout, self.swapchain.format)?;
            self.present_pipeline = create_fullscreen_pipeline(
                &self.device,
                self.fullscreen_layout,
                self.swapchain.format,
            )?;
        }
        self.needs_recreate = false;
        Ok(())
    }

    /// Draws one frame and shows it.
    pub fn render(&mut self, frame: &Frame) -> Result<(), RenderError> {
        unsafe {
            if self.needs_recreate || self.swapchain.handle == vk::SwapchainKHR::null() {
                self.recreate_swapchain()?;
            }
            if self.swapchain.handle == vk::SwapchainKHR::null() {
                return Ok(()); // The window has no area, for example while minimised.
            }

            let slot = self.frame_index;
            let fence = self.frames[slot].fence;
            self.device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(vk_err("waiting for the previous frame"))?;

            let image_index = match self.swapchain_loader.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                self.frames[slot].image_available,
                vk::Fence::null(),
            ) {
                Ok((index, suboptimal)) => {
                    self.needs_recreate |= suboptimal;
                    index as usize
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    self.needs_recreate = true;
                    return Ok(());
                }
                Err(result) => return Err(vk_err("getting the next image")(result)),
            };
            self.device
                .reset_fences(&[fence])
                .map_err(vk_err("resetting a fence"))?;

            // Build the instance data and the list of draws.
            let (instances, layers) = self.build_draws(frame);
            if instances.len() > self.frames[slot].capacity {
                if let Some(old) = self.frames[slot].instances.take() {
                    self.destroy_buffer(old);
                }
                let capacity = instances.len().next_power_of_two().max(1024);
                let buffer = self.create_buffer(
                    (capacity * std::mem::size_of::<Instance>()) as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                    MemoryLocation::CpuToGpu,
                    "quad instances",
                )?;
                self.frames[slot].instances = Some(buffer);
                self.frames[slot].capacity = capacity;
            }
            if let Some(buffer) = &mut self.frames[slot].instances {
                let bytes: &[u8] = bytemuck::cast_slice(&instances);
                buffer
                    .allocation
                    .mapped_slice_mut()
                    .expect("instance buffers are mapped")[..bytes.len()]
                    .copy_from_slice(bytes);
            }

            if !frame.ui.is_empty() {
                self.ensure_offscreen()?;
            }
            let (mesh_vertices, mesh_indices, meshes) = self.build_meshes(frame);
            let mut grow = std::mem::take(&mut self.frames[slot].mesh_vertices);
            let filled = self.fill(
                &mut grow,
                bytemuck::cast_slice(&mesh_vertices),
                vk::BufferUsageFlags::VERTEX_BUFFER,
                "ui vertices",
            );
            self.frames[slot].mesh_vertices = grow;
            filled?;
            let mut grow = std::mem::take(&mut self.frames[slot].mesh_indices);
            let filled = self.fill(
                &mut grow,
                bytemuck::cast_slice(&mesh_indices),
                vk::BufferUsageFlags::INDEX_BUFFER,
                "ui indices",
            );
            self.frames[slot].mesh_indices = grow;
            filled?;

            let capture = self.capture_requested && self.swapchain.can_capture;
            if self.capture_requested && !self.swapchain.can_capture {
                log::warn!("this window surface does not allow reading back frames, so no capture was taken");
                self.capture_requested = false;
            }
            let extent = self.swapchain.extent;
            let capture_buffer = if capture {
                Some(self.create_buffer(
                    extent.width as u64 * extent.height as u64 * 4,
                    vk::BufferUsageFlags::TRANSFER_DST,
                    MemoryLocation::GpuToCpu,
                    "frame capture",
                )?)
            } else {
                None
            };

            let cmd = self.frames[slot].cmd;
            let image = self.swapchain.images[image_index];
            self.record(
                frame,
                &layers,
                &meshes,
                slot,
                image_index,
                capture_buffer.as_ref().map(|b| b.buffer),
            )
            .map_err(vk_err("recording the frame"))?;

            let rendered = self.swapchain.rendered[image_index];
            let wait = [self.frames[slot].image_available];
            let signal = [rendered];
            let cmds = [cmd];
            let submit = vk::SubmitInfo::default()
                .wait_semaphores(&wait)
                .wait_dst_stage_mask(&[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT])
                .command_buffers(&cmds)
                .signal_semaphores(&signal);
            self.device
                .queue_submit(self.queue, &[submit], fence)
                .map_err(vk_err("submitting the frame"))?;
            let _ = image;

            if let Some(buffer) = capture_buffer {
                self.device
                    .wait_for_fences(&[fence], true, u64::MAX)
                    .map_err(vk_err("waiting for the capture"))?;
                let mut rgba = buffer
                    .allocation
                    .mapped_slice()
                    .expect("capture buffers are mapped")
                    [..(extent.width * extent.height * 4) as usize]
                    .to_vec();
                if matches!(
                    self.swapchain.format,
                    vk::Format::B8G8R8A8_SRGB | vk::Format::B8G8R8A8_UNORM
                ) {
                    for pixel in rgba.chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                }
                for pixel in rgba.chunks_exact_mut(4) {
                    pixel[3] = 255;
                }
                self.captured = Some(ImageData {
                    width: extent.width,
                    height: extent.height,
                    rgba,
                });
                self.capture_requested = false;
                self.destroy_buffer(buffer);
            }

            let swapchains = [self.swapchain.handle];
            let indices = [image_index as u32];
            let present = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal)
                .swapchains(&swapchains)
                .image_indices(&indices);
            match self.swapchain_loader.queue_present(self.queue, &present) {
                Ok(suboptimal) => self.needs_recreate |= suboptimal,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
                Err(result) => return Err(vk_err("showing the frame")(result)),
            }
            self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
            Ok(())
        }
    }

    /// Copies `bytes` into a per-frame buffer, replacing the buffer with a
    /// bigger one first if needed.
    unsafe fn fill(
        &mut self,
        grow: &mut GrowBuffer,
        bytes: &[u8],
        usage: vk::BufferUsageFlags,
        name: &str,
    ) -> Result<(), RenderError> {
        if bytes.len() > grow.capacity {
            if let Some(old) = grow.buffer.take() {
                self.destroy_buffer(old);
            }
            grow.capacity = 0;
            let capacity = bytes.len().next_power_of_two().max(1 << 16);
            grow.buffer =
                Some(self.create_buffer(capacity as u64, usage, MemoryLocation::CpuToGpu, name)?);
            grow.capacity = capacity;
        }
        if let Some(buffer) = &mut grow.buffer {
            buffer
                .allocation
                .mapped_slice_mut()
                .expect("per-frame buffers are mapped")[..bytes.len()]
                .copy_from_slice(bytes);
        }
        Ok(())
    }

    /// Joins the frame's UI meshes into one vertex and one index list, and
    /// works out each mesh's clip rectangle in pixels.
    fn build_meshes(&self, frame: &Frame) -> (Vec<MeshVertex>, Vec<u32>, Vec<MeshDraw>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut draws = Vec::new();
        let extent = self.swapchain.extent;
        let size = frame.size.max(glam::Vec2::ONE);
        let pixels = glam::Vec2::new(extent.width as f32, extent.height as f32) / size;
        for mesh in &frame.ui {
            let live = mesh.texture.index == BACKDROP_INDEX
                || self
                    .textures
                    .get(mesh.texture.index as usize)
                    .is_some_and(Option::is_some);
            if !live || mesh.indices.is_empty() {
                continue;
            }
            let scissor = match mesh.clip {
                None => vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                },
                Some([x, y, w, h]) => {
                    let left = ((x * pixels.x).round() as i64).clamp(0, extent.width as i64);
                    let top = ((y * pixels.y).round() as i64).clamp(0, extent.height as i64);
                    let right =
                        (((x + w) * pixels.x).round() as i64).clamp(left, extent.width as i64);
                    let bottom =
                        (((y + h) * pixels.y).round() as i64).clamp(top, extent.height as i64);
                    vk::Rect2D {
                        offset: vk::Offset2D {
                            x: left as i32,
                            y: top as i32,
                        },
                        extent: vk::Extent2D {
                            width: (right - left) as u32,
                            height: (bottom - top) as u32,
                        },
                    }
                }
            };
            if scissor.extent.width == 0 || scissor.extent.height == 0 {
                continue;
            }
            draws.push(MeshDraw {
                texture: mesh.texture.index,
                filter: mesh.filter,
                first_index: indices.len() as u32,
                index_count: mesh.indices.len() as u32,
                vertex_offset: vertices.len() as i32,
                scissor,
            });
            vertices.extend_from_slice(&mesh.vertices);
            indices.extend_from_slice(&mesh.indices);
        }
        (vertices, indices, draws)
    }

    /// Turns the frame's quads into instance data and batches. Quads are
    /// sorted by `z` (keeping their order otherwise), then runs of quads
    /// with the same texture become one draw.
    fn build_draws(&self, frame: &Frame) -> (Vec<Instance>, Vec<LayerDraw>) {
        let total: usize = frame.layers.iter().map(|l| l.quads.len()).sum();
        let mut instances = Vec::with_capacity(total);
        let mut layers = Vec::with_capacity(frame.layers.len());
        let clip_from_frame = clip_from_frame(frame);

        for layer in &frame.layers {
            let mut order: Vec<u32> = (0..layer.quads.len() as u32).collect();
            order.sort_by_key(|i| layer.quads[*i as usize].z);
            let mut batches: Vec<Batch> = Vec::new();
            for i in order {
                let quad = &layer.quads[i as usize];
                let live = self
                    .textures
                    .get(quad.texture.index as usize)
                    .is_some_and(Option::is_some);
                if !live {
                    continue;
                }
                let [r, g, b, a] = quad.color.to_linear();
                let index = instances.len() as u32;
                instances.push(Instance {
                    axes: [quad.x_axis.x, quad.x_axis.y, quad.y_axis.x, quad.y_axis.y],
                    origin: quad.origin.to_array(),
                    uv: quad.uv,
                    color: [r * a, g * a, b * a, a],
                });
                match batches.last_mut() {
                    Some(batch)
                        if batch.texture == quad.texture.index && batch.filter == quad.filter =>
                    {
                        batch.count += 1
                    }
                    _ => batches.push(Batch {
                        texture: quad.texture.index,
                        filter: quad.filter,
                        first: index,
                        count: 1,
                    }),
                }
            }
            let v = layer.view;
            let view = Mat4::from_cols_array(&[
                v.matrix2.x_axis.x,
                v.matrix2.x_axis.y,
                0.0,
                0.0,
                v.matrix2.y_axis.x,
                v.matrix2.y_axis.y,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                v.translation.x,
                v.translation.y,
                0.0,
                1.0,
            ]);
            layers.push(LayerDraw {
                clip_from_world: clip_from_frame * view,
                batches,
            });
        }
        (instances, layers)
    }

    unsafe fn record(
        &self,
        frame: &Frame,
        layers: &[LayerDraw],
        meshes: &[MeshDraw],
        slot: usize,
        image_index: usize,
        capture_into: Option<vk::Buffer>,
    ) -> Result<(), vk::Result> {
        let device = &self.device;
        let cmd = self.frames[slot].cmd;
        let image = self.swapchain.images[image_index];
        let extent = self.swapchain.extent;

        device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
        device.begin_command_buffer(
            cmd,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        let [r, g, b, _] = frame.clear.to_linear();
        let clear = [r, g, b, 1.0];
        let offscreen = self.offscreen.as_ref().filter(|_| !meshes.is_empty());

        self.barrier(
            cmd,
            image,
            (
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            ),
            (
                vk::AccessFlags::empty(),
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            ),
            (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ),
        );
        match offscreen {
            None => {
                self.begin_pass(cmd, self.swapchain.views[image_index], extent, Some(clear));
                self.draw_layers(cmd, layers, slot, self.pipeline);
                self.rendering.cmd_end_rendering(cmd);
            }
            Some(offscreen) => {
                // The layers go to an image of their own first, so that a
                // blurred copy can be made for the UI's glass. The UI is then
                // drawn in gamma space and the result copied to the window.
                let scene = &offscreen.scene;
                self.to_target(cmd, scene.image, vk::ImageLayout::UNDEFINED);
                self.begin_pass(cmd, scene.view, extent, Some(clear));
                self.draw_layers(cmd, layers, slot, self.offscreen_pipeline);
                self.rendering.cmd_end_rendering(cmd);
                self.to_sampled(cmd, scene.image);
                let glass = meshes.iter().any(|m| m.texture == BACKDROP_INDEX);
                if glass {
                    self.blur(cmd, offscreen);
                }

                let ui = &offscreen.ui;
                self.to_target(cmd, ui.image, vk::ImageLayout::UNDEFINED);
                self.begin_pass(cmd, ui.view, extent, None);
                self.fullscreen(
                    cmd,
                    self.compose_pipeline,
                    scene.set,
                    FullscreenParams::encode(),
                );
                self.draw_meshes(cmd, frame, meshes, slot, glass.then_some(offscreen));
                self.rendering.cmd_end_rendering(cmd);
                self.to_sampled(cmd, ui.image);

                self.begin_pass(cmd, self.swapchain.views[image_index], extent, None);
                self.fullscreen(
                    cmd,
                    self.present_pipeline,
                    ui.set,
                    FullscreenParams::decode(),
                );
                self.rendering.cmd_end_rendering(cmd);
            }
        }

        match capture_into {
            Some(buffer) => {
                self.barrier(
                    cmd,
                    image,
                    (
                        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    ),
                    (
                        vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                        vk::AccessFlags::TRANSFER_READ,
                    ),
                    (
                        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                        vk::PipelineStageFlags::TRANSFER,
                    ),
                );
                device.cmd_copy_image_to_buffer(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(color_layers())
                        .image_extent(vk::Extent3D {
                            width: extent.width,
                            height: extent.height,
                            depth: 1,
                        })],
                );
                self.barrier(
                    cmd,
                    image,
                    (
                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                        vk::ImageLayout::PRESENT_SRC_KHR,
                    ),
                    (vk::AccessFlags::TRANSFER_READ, vk::AccessFlags::empty()),
                    (
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    ),
                );
            }
            None => self.barrier(
                cmd,
                image,
                (
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::PRESENT_SRC_KHR,
                ),
                (
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                    vk::AccessFlags::empty(),
                ),
                (
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                ),
            ),
        }
        device.end_command_buffer(cmd)
    }

    /// Starts drawing into `view`, clearing it first if `clear` is given,
    /// and sets the viewport and scissor to cover it.
    unsafe fn begin_pass(
        &self,
        cmd: vk::CommandBuffer,
        view: vk::ImageView,
        extent: vk::Extent2D,
        clear: Option<[f32; 4]>,
    ) {
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(if clear.is_some() {
                vk::AttachmentLoadOp::CLEAR
            } else {
                vk::AttachmentLoadOp::DONT_CARE
            })
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: clear.unwrap_or_default(),
                },
            })];
        let area = vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        };
        self.rendering.cmd_begin_rendering(
            cmd,
            &vk::RenderingInfo::default()
                .render_area(area)
                .layer_count(1)
                .color_attachments(&attachments),
        );
        self.device.cmd_set_viewport(
            cmd,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        self.device.cmd_set_scissor(cmd, 0, &[area]);
    }

    /// Readies an offscreen image to be drawn into. Its old contents are
    /// dropped unless `from` says what layout they are in.
    unsafe fn to_target(&self, cmd: vk::CommandBuffer, image: vk::Image, from: vk::ImageLayout) {
        // The previous frame may still be reading it.
        self.barrier(
            cmd,
            image,
            (from, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
            (
                vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            ),
            (
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ),
        );
    }

    /// Readies an offscreen image that was just drawn into to be read.
    unsafe fn to_sampled(&self, cmd: vk::CommandBuffer, image: vk::Image) {
        self.barrier(
            cmd,
            image,
            (
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ),
            (
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                vk::AccessFlags::SHADER_READ,
            ),
            (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
            ),
        );
    }

    /// Blurs the backdrop's scene image into its first level: each step
    /// halves the size, then each step back doubles it again.
    unsafe fn blur(&self, cmd: vk::CommandBuffer, offscreen: &Offscreen) {
        let mut source = &offscreen.scene;
        for level in &offscreen.levels {
            self.to_target(cmd, level.image, vk::ImageLayout::UNDEFINED);
            self.begin_pass(cmd, level.view, level.extent(), None);
            self.fullscreen(
                cmd,
                self.blur_pipeline,
                source.set,
                FullscreenParams::shrink(source),
            );
            self.rendering.cmd_end_rendering(cmd);
            self.to_sampled(cmd, level.image);
            source = level;
        }
        for index in (0..offscreen.levels.len().saturating_sub(1)).rev() {
            let level = &offscreen.levels[index];
            let source = &offscreen.levels[index + 1];
            self.to_target(cmd, level.image, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
            self.begin_pass(cmd, level.view, level.extent(), None);
            let mut params = FullscreenParams::grow(source);
            if index == 0 {
                params.saturation = BACKDROP_SATURATION;
            }
            self.fullscreen(cmd, self.blur_pipeline, source.set, params);
            self.rendering.cmd_end_rendering(cmd);
            self.to_sampled(cmd, level.image);
        }
    }

    /// Covers the current pass with `source`, filtered as `params` says.
    unsafe fn fullscreen(
        &self,
        cmd: vk::CommandBuffer,
        pipeline: vk::Pipeline,
        source: vk::DescriptorSet,
        params: FullscreenParams,
    ) {
        let device = &self.device;
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            self.fullscreen_layout,
            0,
            &[self.sampler_sets[0], source],
            &[],
        );
        device.cmd_push_constants(
            cmd,
            self.fullscreen_layout,
            vk::ShaderStageFlags::FRAGMENT,
            0,
            bytemuck::bytes_of(&params),
        );
        device.cmd_draw(cmd, 3, 1, 0, 0);
    }

    unsafe fn draw_layers(
        &self,
        cmd: vk::CommandBuffer,
        layers: &[LayerDraw],
        slot: usize,
        pipeline: vk::Pipeline,
    ) {
        let device = &self.device;
        let Some(instances) = &self.frames[slot].instances else {
            return;
        };
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_bind_vertex_buffers(cmd, 0, &[instances.buffer], &[0]);
        for layer in layers {
            if layer.batches.is_empty() {
                continue;
            }
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::cast_slice(&layer.clip_from_world.to_cols_array()),
            );
            for batch in &layer.batches {
                let Some(texture) = self.textures[batch.texture as usize].as_ref() else {
                    continue;
                };
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout,
                    0,
                    &[self.sampler_set(batch.filter), texture.set],
                    &[],
                );
                device.cmd_draw(cmd, 6, batch.count, 0, batch.first);
            }
        }
    }

    unsafe fn draw_meshes(
        &self,
        cmd: vk::CommandBuffer,
        frame: &Frame,
        meshes: &[MeshDraw],
        slot: usize,
        backdrop: Option<&Offscreen>,
    ) {
        let device = &self.device;
        let frame_slot = &self.frames[slot];
        let (Some(vertices), Some(indices), false) = (
            &frame_slot.mesh_vertices.buffer,
            &frame_slot.mesh_indices.buffer,
            meshes.is_empty(),
        ) else {
            return;
        };
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.mesh_pipeline);
        device.cmd_bind_vertex_buffers(cmd, 0, &[vertices.buffer], &[0]);
        device.cmd_bind_index_buffer(cmd, indices.buffer, 0, vk::IndexType::UINT32);
        device.cmd_push_constants(
            cmd,
            self.pipeline_layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytemuck::cast_slice(&clip_from_frame(frame).to_cols_array()),
        );
        for mesh in meshes {
            let set = if mesh.texture == BACKDROP_INDEX {
                // Without a backdrop, glass falls back to its plain tint.
                match backdrop {
                    Some(backdrop) => backdrop.levels[0].set,
                    None => match self.textures[self.white.index as usize].as_ref() {
                        Some(white) => white.set,
                        None => continue,
                    },
                }
            } else {
                match self.textures[mesh.texture as usize].as_ref() {
                    Some(texture) => texture.set,
                    None => continue,
                }
            };
            device.cmd_set_scissor(cmd, 0, &[mesh.scissor]);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.sampler_set(mesh.filter), set],
                &[],
            );
            device.cmd_draw_indexed(
                cmd,
                mesh.index_count,
                1,
                mesh.first_index,
                mesh.vertex_offset,
                0,
            );
        }
    }

    fn sampler_set(&self, filter: Filter) -> vk::DescriptorSet {
        self.sampler_sets[match filter {
            Filter::Smooth => 0,
            Filter::Pixelated => 1,
        }]
    }

    // --- Backdrop ------------------------------------------------------------

    /// A texture holding a blurred copy of the frame's layers, for drawing
    /// frosted glass under user interfaces. Use it in a [`Mesh`](crate::Mesh)
    /// with texture coordinates that map the window from `(0, 0)` to
    /// `(1, 1)`. It costs nothing in frames that do not use it.
    pub fn backdrop(&self) -> Texture {
        Texture {
            index: BACKDROP_INDEX,
            width: self.swapchain.extent.width,
            height: self.swapchain.extent.height,
        }
    }

    /// Makes the offscreen images if they are missing or the wrong size.
    unsafe fn ensure_offscreen(&mut self) -> Result<(), RenderError> {
        let extent = self.swapchain.extent;
        if self.offscreen.as_ref().is_some_and(|o| o.extent == extent) {
            return Ok(());
        }
        if let Some(old) = self.offscreen.take() {
            self.device
                .device_wait_idle()
                .map_err(vk_err("waiting for the device"))?;
            self.destroy_offscreen(old);
        }
        while self.offscreen_sets.len() < BLUR_LEVELS + 2 {
            let set = self.allocate_set(self.texture_layout)?;
            self.offscreen_sets.push(set);
        }
        let sets = self.offscreen_sets.clone();
        let (width, height) = (extent.width, extent.height);
        let scene = self.create_target(width, height, OFFSCREEN_FORMAT, sets[0])?;
        let ui = self.create_target(width, height, UI_FORMAT, sets[1])?;
        let mut levels = Vec::new();
        for level in 0..BLUR_LEVELS {
            let shift = level as u32 + 1;
            let target = self.create_target(
                (width >> shift).max(1),
                (height >> shift).max(1),
                OFFSCREEN_FORMAT,
                sets[level + 2],
            )?;
            levels.push(target);
        }
        self.offscreen = Some(Offscreen {
            scene,
            ui,
            levels,
            extent,
        });
        Ok(())
    }

    unsafe fn create_target(
        &mut self,
        width: u32,
        height: u32,
        format: vk::Format,
        set: vk::DescriptorSet,
    ) -> Result<Target, RenderError> {
        let image = self
            .device
            .create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )
            .map_err(vk_err("creating an offscreen image"))?;
        let allocation = self
            .allocator
            .allocate(&AllocationCreateDesc {
                name: "offscreen",
                requirements: self.device.get_image_memory_requirements(image),
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| RenderError::Memory(e.to_string()))?;
        self.device
            .bind_image_memory(image, allocation.memory(), allocation.offset())
            .map_err(vk_err("binding offscreen memory"))?;
        let view = self
            .device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )
            .map_err(vk_err("creating an offscreen view"))?;
        let info = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        self.device.update_descriptor_sets(
            &[vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&info)],
            &[],
        );
        Ok(Target {
            image,
            view,
            allocation,
            set,
            width,
            height,
        })
    }

    /// Frees the offscreen images. The device must be idle.
    unsafe fn destroy_offscreen(&mut self, offscreen: Offscreen) {
        let targets = [offscreen.scene, offscreen.ui]
            .into_iter()
            .chain(offscreen.levels);
        for target in targets {
            self.device.destroy_image_view(target.view, None);
            self.device.destroy_image(target.image, None);
            let _ = self.allocator.free(target.allocation);
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            for texture in std::mem::take(&mut self.textures).into_iter().flatten() {
                self.free_texture(texture);
            }
            if let Some(offscreen) = self.offscreen.take() {
                self.destroy_offscreen(offscreen);
            }
            for slot in std::mem::take(&mut self.frames) {
                for buffer in [
                    slot.instances,
                    slot.mesh_vertices.buffer,
                    slot.mesh_indices.buffer,
                ]
                .into_iter()
                .flatten()
                {
                    self.destroy_buffer(buffer);
                }
                self.device.destroy_fence(slot.fence, None);
                self.device.destroy_semaphore(slot.image_available, None);
            }
            for pool in &self.descriptor_pools {
                self.device.destroy_descriptor_pool(*pool, None);
            }
            for sampler in self.samplers {
                self.device.destroy_sampler(sampler, None);
            }
            self.device.destroy_pipeline(self.mesh_pipeline, None);
            self.device.destroy_pipeline(self.pipeline, None);
            for pipeline in [
                self.offscreen_pipeline,
                self.blur_pipeline,
                self.compose_pipeline,
                self.present_pipeline,
            ] {
                self.device.destroy_pipeline(pipeline, None);
            }
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_pipeline_layout(self.fullscreen_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.sampler_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.texture_layout, None);
            for view in &self.swapchain.views {
                self.device.destroy_image_view(*view, None);
            }
            for semaphore in &self.swapchain.rendered {
                self.device.destroy_semaphore(*semaphore, None);
            }
            self.swapchain_loader
                .destroy_swapchain(self.swapchain.handle, None);
            self.device.destroy_command_pool(self.pool, None);
            ManuallyDrop::drop(&mut self.allocator);
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
        let _ = &self.entry;
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

fn color_layers() -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .layer_count(1)
}

/// Picks the best graphics device that can draw to the surface.
unsafe fn pick_device(
    instance: &ash::Instance,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32, String, bool), RenderError> {
    let devices = instance
        .enumerate_physical_devices()
        .map_err(vk_err("listing graphics devices"))?;
    if devices.is_empty() {
        return Err(RenderError::NoDevice(
            "Vulkan found no graphics devices".into(),
        ));
    }
    let mut best: Option<(i32, vk::PhysicalDevice, u32, String, bool)> = None;
    let mut reasons = Vec::new();
    for device in devices {
        let props = instance.get_physical_device_properties(device);
        let name = props
            .device_name_as_c_str()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if props.api_version < vk::API_VERSION_1_2 {
            reasons.push(format!(
                "{name}: only supports Vulkan {}.{}",
                vk::api_version_major(props.api_version),
                vk::api_version_minor(props.api_version)
            ));
            continue;
        }
        let extensions = instance
            .enumerate_device_extension_properties(device)
            .unwrap_or_default();
        let has = |wanted: &CStr| {
            extensions
                .iter()
                .any(|e| e.extension_name_as_c_str().is_ok_and(|n| n == wanted))
        };
        if !has(khr::swapchain::NAME) || !has(khr::dynamic_rendering::NAME) {
            reasons.push(format!("{name}: is missing a required Vulkan extension"));
            continue;
        }
        let mut dynamic_rendering = vk::PhysicalDeviceDynamicRenderingFeatures::default();
        {
            let mut features =
                vk::PhysicalDeviceFeatures2::default().push_next(&mut dynamic_rendering);
            instance.get_physical_device_features2(device, &mut features);
        }
        if dynamic_rendering.dynamic_rendering != vk::TRUE {
            reasons.push(format!("{name}: does not support dynamic rendering"));
            continue;
        }
        let families = instance.get_physical_device_queue_family_properties(device);
        let family = families.iter().enumerate().position(|(index, family)| {
            family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
                && surface_loader
                    .get_physical_device_surface_support(device, index as u32, surface)
                    .unwrap_or(false)
        });
        let Some(family) = family else {
            reasons.push(format!("{name}: cannot draw to this window"));
            continue;
        };
        let score = match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 4,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 3,
            vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
            _ => 1,
        };
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((
                score,
                device,
                family as u32,
                name,
                has(khr::portability_subset::NAME),
            ));
        }
    }
    best.map(|(_, device, family, name, portability)| (device, family, name, portability))
        .ok_or_else(|| RenderError::NoDevice(reasons.join("; ")))
}

unsafe fn create_swapchain(
    device: &ash::Device,
    surface_loader: &khr::surface::Instance,
    swapchain_loader: &khr::swapchain::Device,
    physical: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    wanted: (u32, u32),
    old: vk::SwapchainKHR,
) -> Result<Swapchain, RenderError> {
    let caps = surface_loader
        .get_physical_device_surface_capabilities(physical, surface)
        .map_err(vk_err("reading the window surface"))?;
    let formats = surface_loader
        .get_physical_device_surface_formats(physical, surface)
        .map_err(vk_err("reading the window surface formats"))?;
    let format = formats
        .iter()
        .find(|f| {
            matches!(
                f.format,
                vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
            ) && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .or(formats.first())
        .copied()
        .ok_or_else(|| RenderError::NoDevice("the window surface has no formats".into()))?;
    if !matches!(
        format.format,
        vk::Format::B8G8R8A8_SRGB | vk::Format::R8G8B8A8_SRGB
    ) {
        log::warn!("the window does not offer an sRGB format, so colours may look too dark");
    }

    let extent = if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D {
            width: wanted
                .0
                .clamp(caps.min_image_extent.width, caps.max_image_extent.width),
            height: wanted
                .1
                .clamp(caps.min_image_extent.height, caps.max_image_extent.height),
        }
    };
    let empty = Swapchain {
        handle: vk::SwapchainKHR::null(),
        format: format.format,
        extent,
        images: Vec::new(),
        views: Vec::new(),
        rendered: Vec::new(),
        can_capture: false,
    };
    if extent.width == 0 || extent.height == 0 {
        return Ok(empty);
    }

    let mut image_count = caps.min_image_count + 1;
    if caps.max_image_count > 0 {
        image_count = image_count.min(caps.max_image_count);
    }
    let can_capture = caps
        .supported_usage_flags
        .contains(vk::ImageUsageFlags::TRANSFER_SRC);
    let mut usage = vk::ImageUsageFlags::COLOR_ATTACHMENT;
    if can_capture {
        usage |= vk::ImageUsageFlags::TRANSFER_SRC;
    }
    let alpha = [
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::INHERIT,
    ]
    .into_iter()
    .find(|a| caps.supported_composite_alpha.contains(*a))
    .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE);
    let info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(format.format)
        .image_color_space(format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(usage)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(caps.current_transform)
        .composite_alpha(alpha)
        .present_mode(vk::PresentModeKHR::FIFO)
        .clipped(true)
        .old_swapchain(old);
    let handle = swapchain_loader
        .create_swapchain(&info, None)
        .map_err(vk_err("creating the swapchain"))?;
    let images = swapchain_loader
        .get_swapchain_images(handle)
        .map_err(vk_err("getting the swapchain images"))?;
    let mut views = Vec::new();
    let mut rendered = Vec::new();
    for image in &images {
        views.push(
            device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(*image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format.format)
                        .subresource_range(color_range()),
                    None,
                )
                .map_err(vk_err("creating a swapchain view"))?,
        );
        rendered.push(
            device
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                .map_err(vk_err("creating a semaphore"))?,
        );
    }
    Ok(Swapchain {
        handle,
        images,
        views,
        rendered,
        can_capture,
        ..empty
    })
}

/// Frame units to clip space: `(0, 0)` is the top-left corner.
fn clip_from_frame(frame: &Frame) -> Mat4 {
    let size = frame.size.max(glam::Vec2::ONE);
    Mat4::from_translation(Vec3::new(-1.0, -1.0, 0.0))
        * Mat4::from_scale(Vec3::new(2.0 / size.x, 2.0 / size.y, 1.0))
}

/// The pipeline that draws instanced quads: sprites and text.
unsafe fn create_quad_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    color_format: vk::Format,
) -> Result<vk::Pipeline, RenderError> {
    let attribute = |location: u32, format: vk::Format, offset: u32| {
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(location)
            .format(format)
            .offset(offset)
    };
    create_pipeline(
        device,
        layout,
        color_format,
        QUAD_WGSL,
        true,
        Some(
            vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(std::mem::size_of::<Instance>() as u32)
                .input_rate(vk::VertexInputRate::INSTANCE),
        ),
        &[
            attribute(0, vk::Format::R32G32B32A32_SFLOAT, 0),
            attribute(1, vk::Format::R32G32_SFLOAT, 16),
            attribute(2, vk::Format::R32G32B32A32_SFLOAT, 24),
            attribute(3, vk::Format::R32G32B32A32_SFLOAT, 40),
        ],
    )
}

/// The pipeline that draws indexed triangle meshes: the editor's UI.
unsafe fn create_mesh_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    color_format: vk::Format,
) -> Result<vk::Pipeline, RenderError> {
    let attribute = |location: u32, format: vk::Format, offset: u32| {
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(location)
            .format(format)
            .offset(offset)
    };
    create_pipeline(
        device,
        layout,
        color_format,
        MESH_WGSL,
        true,
        Some(
            vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(std::mem::size_of::<MeshVertex>() as u32)
                .input_rate(vk::VertexInputRate::VERTEX),
        ),
        &[
            attribute(0, vk::Format::R32G32_SFLOAT, 0),
            attribute(1, vk::Format::R32G32_SFLOAT, 8),
            attribute(2, vk::Format::R8G8B8A8_UNORM, 16),
        ],
    )
}

/// The pipeline for full-window passes: blurring and copying the backdrop.
unsafe fn create_fullscreen_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    color_format: vk::Format,
) -> Result<vk::Pipeline, RenderError> {
    create_pipeline(
        device,
        layout,
        color_format,
        FULLSCREEN_WGSL,
        false,
        None,
        &[],
    )
}

unsafe fn create_pipeline(
    device: &ash::Device,
    layout: vk::PipelineLayout,
    color_format: vk::Format,
    shader: &str,
    blend: bool,
    binding: Option<vk::VertexInputBindingDescription>,
    attributes: &[vk::VertexInputAttributeDescription],
) -> Result<vk::Pipeline, RenderError> {
    let spirv = compile_wgsl(shader)?;
    let module = device
        .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv), None)
        .map_err(vk_err("creating the shader"))?;
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(module)
            .name(c"vs_main"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(module)
            .name(c"fs_main"),
    ];
    let bindings: Vec<_> = binding.into_iter().collect();
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    // Premultiplied alpha blending, or none: the pass replaces what was there.
    let blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(blend)
        .src_color_blend_factor(vk::BlendFactor::ONE)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);
    let dynamic = vk::PipelineDynamicStateCreateInfo::default()
        .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
    let formats = [color_format];
    let mut rendering =
        vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .push_next(&mut rendering);
    let result = device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None);
    device.destroy_shader_module(module, None);
    result
        .map(|pipelines| pipelines[0])
        .map_err(|(_, result)| vk_err("creating the pipeline")(result))
}

/// Copies straight-alpha sRGB pixels into `out` as premultiplied sRGB, which
/// is what the shader and blending expect. Premultiplying happens in linear
/// light so soft edges do not get dark fringes.
fn premultiply_into(rgba: &[u8], out: &mut [u8]) {
    fn to_linear(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn to_srgb(c: f32) -> u8 {
        let c = if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    }
    for (src, dst) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        match src[3] {
            255 => dst.copy_from_slice(src),
            0 => dst.copy_from_slice(&[0, 0, 0, 0]),
            alpha => {
                let a = alpha as f32 / 255.0;
                dst[0] = to_srgb(to_linear(src[0]) * a);
                dst[1] = to_srgb(to_linear(src[1]) * a);
                dst[2] = to_srgb(to_linear(src[2]) * a);
                dst[3] = alpha;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_keeps_opaque_and_clears_transparent() {
        let src = [10, 20, 30, 255, 200, 100, 50, 0, 255, 255, 255, 128];
        let mut out = [0u8; 12];
        premultiply_into(&src, &mut out);
        assert_eq!(&out[0..4], &[10, 20, 30, 255]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        // Half-transparent white: linear 0.5 is about 188 in sRGB.
        assert_eq!(out[11], 128);
        assert!((186..=189).contains(&out[8]), "{}", out[8]);
    }

    #[test]
    fn instance_layout_matches_the_pipeline() {
        assert_eq!(std::mem::size_of::<Instance>(), 56);
        assert_eq!(std::mem::size_of::<MeshVertex>(), 20);
    }
}
