//! Renderer configuration: backend, adapter, present mode, frame latency, and
//! DX12 shader compiler. One `RenderSettings` value holds every knob; the
//! mapping helpers translate to wgpu types with sensible fallbacks.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendChoice {
    Auto,
    Dx12,
    Vulkan,
    Metal,
    Opengl,
}

impl BackendChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Dx12 => "DirectX 12",
            Self::Vulkan => "Vulkan",
            Self::Metal => "Metal",
            Self::Opengl => "OpenGL",
        }
    }

    /// Backends that make sense to surface in the UI on the current platform.
    pub fn ui_choices() -> &'static [Self] {
        #[cfg(target_os = "windows")]
        {
            &[Self::Auto, Self::Dx12, Self::Vulkan, Self::Opengl]
        }
        #[cfg(target_os = "macos")]
        {
            &[Self::Auto, Self::Metal, Self::Vulkan]
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            &[Self::Auto, Self::Vulkan, Self::Opengl]
        }
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value.map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("auto") => Self::Auto,
            Some("dx12") => Self::Dx12,
            Some("vulkan") => Self::Vulkan,
            Some("metal") => Self::Metal,
            Some("opengl" | "gl") => Self::Opengl,
            _ => Self::Auto,
        }
    }

    pub fn to_wgpu(self) -> wgpu::Backends {
        match self {
            Self::Auto => wgpu::Backends::all(),
            Self::Dx12 => wgpu::Backends::DX12,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Metal => wgpu::Backends::METAL,
            Self::Opengl => wgpu::Backends::GL,
        }
    }

    pub fn preferred_backend_order(self) -> &'static [wgpu::Backend] {
        match self {
            Self::Dx12 => &[wgpu::Backend::Dx12],
            Self::Vulkan => &[wgpu::Backend::Vulkan],
            Self::Metal => &[wgpu::Backend::Metal],
            Self::Opengl => &[wgpu::Backend::Gl],
            Self::Auto => {
                #[cfg(target_os = "windows")]
                {
                    &[
                        wgpu::Backend::Dx12,
                        wgpu::Backend::Vulkan,
                        wgpu::Backend::Gl,
                    ]
                }
                #[cfg(target_os = "macos")]
                {
                    &[
                        wgpu::Backend::Metal,
                        wgpu::Backend::Vulkan,
                        wgpu::Backend::Gl,
                    ]
                }
                #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
                {
                    &[wgpu::Backend::Vulkan, wgpu::Backend::Gl]
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentModeChoice {
    Auto,
    Fifo,
    FifoRelaxed,
    Mailbox,
    Immediate,
}

impl PresentModeChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Fifo)",
            Self::Fifo => "Fifo (vsync)",
            Self::FifoRelaxed => "FifoRelaxed",
            Self::Mailbox => "Mailbox (fast vsync)",
            Self::Immediate => "Immediate (uncapped)",
        }
    }

    pub fn ui_choices() -> &'static [Self] {
        &[
            Self::Auto,
            Self::Fifo,
            Self::FifoRelaxed,
            Self::Mailbox,
            Self::Immediate,
        ]
    }

    pub fn to_wgpu(self) -> Option<wgpu::PresentMode> {
        match self {
            Self::Auto => None,
            Self::Fifo => Some(wgpu::PresentMode::Fifo),
            Self::FifoRelaxed => Some(wgpu::PresentMode::FifoRelaxed),
            Self::Mailbox => Some(wgpu::PresentMode::Mailbox),
            Self::Immediate => Some(wgpu::PresentMode::Immediate),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DxCompilerChoice {
    /// wgpu picks: static DXC if linked, then dynamic, then FXC.
    Auto,
    Fxc,
    StaticDxc,
}

impl DxCompilerChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Fxc => "FXC (legacy)",
            Self::StaticDxc => "DXC (modern)",
        }
    }

    pub fn ui_choices() -> &'static [Self] {
        &[Self::Auto, Self::Fxc, Self::StaticDxc]
    }

    pub fn to_wgpu(self) -> wgpu::Dx12Compiler {
        match self {
            Self::Auto => wgpu::Dx12Compiler::Auto,
            Self::Fxc => wgpu::Dx12Compiler::Fxc,
            Self::StaticDxc => wgpu::Dx12Compiler::StaticDxc,
        }
    }
}

/// Every renderer-affecting knob in one place. Present mode and frame latency
/// apply via surface reconfigure; the rest require a full renderer rebuild.
#[derive(Clone, PartialEq, Debug)]
pub struct RenderSettings {
    pub backend: BackendChoice,
    /// None = automatic adapter selection; Some(name) = match by adapter name.
    pub adapter_name: Option<String>,
    pub present_mode: PresentModeChoice,
    /// desired_maximum_frame_latency, clamped 1..=3 by the UI.
    pub frame_latency: u32,
    pub dx12_compiler: DxCompilerChoice,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            backend: BackendChoice::Auto,
            adapter_name: None,
            present_mode: PresentModeChoice::Auto,
            frame_latency: 2,
            dx12_compiler: DxCompilerChoice::Auto,
        }
    }
}

impl RenderSettings {
    /// Resolve the requested present mode against what the surface supports,
    /// falling back Immediate → Mailbox → FifoRelaxed → Fifo.
    pub fn resolve_present_mode(&self, supported: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        let requested = match self.present_mode.to_wgpu() {
            None => return wgpu::PresentMode::Fifo,
            Some(mode) => mode,
        };
        if supported.contains(&requested) {
            return requested;
        }
        for fallback in [
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::FifoRelaxed,
            wgpu::PresentMode::Fifo,
        ] {
            if supported.contains(&fallback) {
                return fallback;
            }
        }
        wgpu::PresentMode::Fifo
    }

    /// True when switching from `previous` to `self` needs a device/instance
    /// rebuild (backend, adapter, or DX12 compiler changed) rather than just a
    /// surface reconfigure.
    pub fn requires_renderer_rebuild(&self, previous: &RenderSettings) -> bool {
        self.backend != previous.backend
            || self.adapter_name != previous.adapter_name
            || self.dx12_compiler != previous.dx12_compiler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_mode_auto_resolves_to_fifo() {
        let s = RenderSettings::default();
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn present_mode_supported_choice_is_used() {
        let mut s = RenderSettings::default();
        s.present_mode = PresentModeChoice::Mailbox;
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Mailbox
        );
    }

    #[test]
    fn present_mode_unsupported_falls_back_in_order() {
        let mut s = RenderSettings::default();
        s.present_mode = PresentModeChoice::Immediate;
        // Immediate unsupported, Mailbox supported → Mailbox.
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox]),
            wgpu::PresentMode::Mailbox
        );
        // Only Fifo supported → Fifo.
        assert_eq!(
            s.resolve_present_mode(&[wgpu::PresentMode::Fifo]),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn rebuild_required_only_for_device_level_changes() {
        let a = RenderSettings::default();

        let mut present_only = a.clone();
        present_only.present_mode = PresentModeChoice::Immediate;
        present_only.frame_latency = 3;
        assert!(!present_only.requires_renderer_rebuild(&a));

        let mut backend = a.clone();
        backend.backend = BackendChoice::Vulkan;
        assert!(backend.requires_renderer_rebuild(&a));

        let mut adapter = a.clone();
        adapter.adapter_name = Some("Radeon".into());
        assert!(adapter.requires_renderer_rebuild(&a));

        let mut compiler = a.clone();
        compiler.dx12_compiler = DxCompilerChoice::StaticDxc;
        assert!(compiler.requires_renderer_rebuild(&a));
    }
}
