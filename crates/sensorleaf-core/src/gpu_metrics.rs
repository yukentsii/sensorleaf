//! Decoder for amdgpu `gpu_metrics` format 3.0 (SMU 14, e.g. Strix Point).
//! Layout follows `struct gpu_metrics_v3_0` in the kernel's kgd_pp_interface.h.

pub struct GpuMetricsV3 {
    pub temperature_gfx: u16,
    pub temperature_soc: u16,
    pub temperature_core: [u16; 16],
    pub temperature_skin: u16,
    pub gfx_activity: u16,
    pub vcn_activity: u16,
    pub ipu_activity: [u16; 8],
    pub core_c0: [u16; 16],
    pub dram_reads: u16,
    pub dram_writes: u16,
    pub ipu_reads: u16,
    pub ipu_writes: u16,
    pub socket_power: u32,
    pub ipu_power: u16,
    pub apu_power: u32,
    pub gfx_power: u32,
    pub dgpu_power: u32,
    pub all_core_power: u32,
    pub core_power: [u16; 16],
    pub sys_power: u16,
    pub stapm_limit: u16,
    pub current_stapm_limit: u16,
    pub gfxclk: u16,
    pub socclk: u16,
    pub vpeclk: u16,
    pub ipuclk: u16,
    pub fclk: u16,
    pub vclk: u16,
    pub uclk: u16,
    pub mpipuclk: u16,
    pub coreclk: [u16; 16],
    pub core_maxfreq: u16,
    pub gfx_maxfreq: u16,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn arr<const N: usize>(d: &[u8], o: usize) -> [u16; N] {
    std::array::from_fn(|i| u16_at(d, o + 2 * i))
}

impl GpuMetricsV3 {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 228 || d[2] != 3 || d[3] != 0 {
            return None;
        }
        Some(GpuMetricsV3 {
            temperature_gfx: u16_at(d, 4),
            temperature_soc: u16_at(d, 6),
            temperature_core: arr(d, 8),
            temperature_skin: u16_at(d, 40),
            gfx_activity: u16_at(d, 42),
            vcn_activity: u16_at(d, 44),
            ipu_activity: arr(d, 46),
            core_c0: arr(d, 62),
            dram_reads: u16_at(d, 94),
            dram_writes: u16_at(d, 96),
            ipu_reads: u16_at(d, 98),
            ipu_writes: u16_at(d, 100),
            socket_power: u32_at(d, 112),
            ipu_power: u16_at(d, 116),
            apu_power: u32_at(d, 120),
            gfx_power: u32_at(d, 124),
            dgpu_power: u32_at(d, 128),
            all_core_power: u32_at(d, 132),
            core_power: arr(d, 136),
            sys_power: u16_at(d, 168),
            stapm_limit: u16_at(d, 170),
            current_stapm_limit: u16_at(d, 172),
            gfxclk: u16_at(d, 174),
            socclk: u16_at(d, 176),
            vpeclk: u16_at(d, 178),
            ipuclk: u16_at(d, 180),
            fclk: u16_at(d, 182),
            vclk: u16_at(d, 184),
            uclk: u16_at(d, 186),
            mpipuclk: u16_at(d, 188),
            coreclk: arr(d, 190),
            core_maxfreq: u16_at(d, 222),
            gfx_maxfreq: u16_at(d, 224),
        })
    }
}

/// 0xFFFF / 0xFFFFFFFF mean "not supported by firmware".
pub fn ok16(v: u16) -> Option<f64> {
    (v != u16::MAX).then_some(v as f64)
}
pub fn ok32(v: u32) -> Option<f64> {
    (v != u32::MAX).then_some(v as f64)
}
