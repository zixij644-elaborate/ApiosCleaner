//! Lipo 瘦身核心 —— fat（universal）Mach-O 二进制瘦身为当前架构单切片
//!
//! 设计要点：
//! 1. 运行时 `cfg!(target_arch)` 选目标架构，不做编译期硬编码
//! 2. 同 cputype 取最高 cpusubtype（arm64e=2 优先于 arm64=0，x86_64h=8 优先于 x86_64=0）
//! 3. 同时支持 fat 与 fat64（magic 0xcafebabf，offset/size 为 u64）
//! 4. 只 seek 读目标切片字节，不整文件读入内存（fat 文件可达几百 MB）
//! 5. 同目录临时文件 + 原子 rename（保留权限位），避免覆盖写回中断即损坏
//! 6. nfat_arch 上限 / 切片越界 / 表截断统一校验
//!
//! 模块归属平台层（`#[cfg(target_os = "macos")]` 门控）：universal（fat）二进制是
//! Darwin 平台独有的格式，其他平台无此结构，非 macOS 构建不编译本模块。
//! 解析/选择/扫描/瘦身均为纯 std，唯一 OS 相关是 ad-hoc 重签（codesign）。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

// ---------- 常量 ----------

/// fat 32 位头 magic（磁盘上恒为大端字节序列 ca fe ba be）
pub const FAT_MAGIC: u32 = 0xcafe_babe;
/// fat 64 位头 magic（大文件：offset/size 为 u64）
pub const FAT_MAGIC_64: u32 = 0xcafe_babf;
/// Mach-O 64 位 / 32 位 magic（及字节序交换变体，仅用于二进制候选判定）
pub const MH_MAGIC: u32 = 0xfeed_face;
pub const MH_CIGAM: u32 = 0xcefa_edfe;
pub const MH_MAGIC_64: u32 = 0xfeed_facf;
pub const MH_CIGAM_64: u32 = 0xcffa_edfe;

/// CPU_TYPE_ARM64 = 0x0100_000C（CPU_ARCH_ABI64 | CPU_TYPE_ARM）
pub const CPU_TYPE_ARM64: u32 = 0x0100_000C;
/// CPU_TYPE_X86_64 = 0x0100_0007（CPU_ARCH_ABI64 | CPU_TYPE_X86）
pub const CPU_TYPE_X86_64: u32 = 0x0100_0007;
/// CPU_SUBTYPE_ARM64E（指针认证变体，优先于普通 arm64 保留）
pub const CPU_SUBTYPE_ARM64E: u32 = 2;
/// CPU_SUBTYPE_X86_64H（Haswell 变体，优先于普通 x86_64）
pub const CPU_SUBTYPE_X86_64H: u32 = 8;

/// nfat_arch 合理上限（防御恶意/损坏文件；真实 fat 不会超过个位数）
const MAX_SLICES: u32 = 32;
/// fat32 切片表条目大小（cputype/subtype/offset/size/align 各 4 字节）
const FAT_ARCH32_SIZE: usize = 20;
/// fat64 切片表条目大小（offset/size 为 8 字节）
const FAT_ARCH64_SIZE: usize = 32;

// ---------- 类型 ----------

/// 一个 fat 架构切片条目
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FatSlice {
    pub cputype: u32,
    pub cpusubtype: u32,
    pub offset: u64,
    pub size: u64,
}

/// 解析后的 fat 文件（切片表）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FatFile {
    pub slices: Vec<FatSlice>,
}

/// fat 解析错误。CLI 层转文本；瘦身/扫描层决定跳过还是报错。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LipError {
    /// 不是 fat 二进制（thin Mach-O 或普通文件）
    NotFat,
    /// 文件不完整（头或切片表截断）
    Truncated(&'static str),
    /// 结构非法（nfat_arch 为 0/超限、切片越界）
    Invalid(&'static str),
}

impl std::fmt::Display for LipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LipError::NotFat => write!(f, "not a universal (fat) binary"),
            LipError::Truncated(what) => write!(f, "truncated file: {what}"),
            LipError::Invalid(what) => write!(f, "invalid fat structure: {what}"),
        }
    }
}

// ---------- 纯解析（fixture 字节单测） ----------

/// 读前 4 字节为 Mach-O magic？（6 种：32/64 位 + 大小端变体）
pub fn is_macho_magic(head: [u8; 4]) -> bool {
    matches!(
        u32::from_be_bytes(head),
        MH_MAGIC | MH_CIGAM | MH_MAGIC_64 | MH_CIGAM_64
    )
}

/// 读前 4 字节为 fat magic？（32 位 + 64 位）
pub fn is_fat_magic(head: [u8; 4]) -> bool {
    matches!(u32::from_be_bytes(head), FAT_MAGIC | FAT_MAGIC_64)
}

/// 解析 fat 文件（完整字节内容）。fat32 与 fat64 都支持。
/// 校验：nfat_arch 范围、切片表不截断、每条切片不越界、size 非零。
pub fn parse_fat(bytes: &[u8]) -> Result<FatFile, LipError> {
    if bytes.len() < 8 {
        return Err(LipError::Truncated("header"));
    }
    let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let entry_size = match magic {
        FAT_MAGIC => FAT_ARCH32_SIZE,
        FAT_MAGIC_64 => FAT_ARCH64_SIZE,
        _ => return Err(LipError::NotFat),
    };
    let nfat = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if nfat == 0 {
        return Err(LipError::Invalid("nfat_arch is zero"));
    }
    if nfat > MAX_SLICES {
        return Err(LipError::Invalid("nfat_arch exceeds sane limit"));
    }

    // 切片表区
    let table_len = nfat as usize * entry_size;
    let table = match bytes.get(8..8 + table_len) {
        Some(t) => t,
        None => return Err(LipError::Truncated("architecture table")),
    };

    parse_table(table, magic == FAT_MAGIC_64, bytes.len() as u64)
}

/// 解析切片表（parse_fat 与扫描版 read_fat_if_candidate 共用）。
/// `file_len` 为文件实际长度（表区可能只读入部分 → 越界校验用真实长度）。
fn parse_table(table: &[u8], fat64: bool, file_len: u64) -> Result<FatFile, LipError> {
    let entry_size = if fat64 {
        FAT_ARCH64_SIZE
    } else {
        FAT_ARCH32_SIZE
    };
    let nfat = table.len() / entry_size;
    let mut slices = Vec::with_capacity(nfat);
    for entry in table.chunks_exact(entry_size) {
        let (cputype, cpusubtype, offset, size) = if fat64 {
            (
                u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]),
                u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]),
                u64::from_be_bytes(entry[8..16].try_into().unwrap()),
                u64::from_be_bytes(entry[16..24].try_into().unwrap()),
            )
        } else {
            (
                u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]),
                u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]),
                u32::from_be_bytes([entry[8], entry[9], entry[10], entry[11]]) as u64,
                u32::from_be_bytes([entry[12], entry[13], entry[14], entry[15]]) as u64,
            )
        };
        if size == 0 {
            return Err(LipError::Invalid("slice size is zero"));
        }
        let end = offset.checked_add(size);
        match end {
            Some(end) if end <= file_len => {}
            Some(_) => return Err(LipError::Invalid("slice out of bounds")),
            None => return Err(LipError::Invalid("slice size overflow")),
        }
        slices.push(FatSlice {
            cputype,
            cpusubtype,
            offset,
            size,
        });
    }

    Ok(FatFile { slices })
}

/// 选择目标切片（纯语义）：匹配 cputype，同 cputype 取最高 cpusubtype
/// （arm64e 优先于 arm64、x86_64h 优先于 x86_64 —— 修原版只取第一个匹配的问题）。
/// 生产路径用 `select_runnable_slice`（额外过滤 CPU 能力）。
pub fn select_slice(slices: &[FatSlice], cputype: u32) -> Option<&FatSlice> {
    slices
        .iter()
        .filter(|s| s.cputype == cputype)
        .max_by_key(|s| s.cpusubtype)
}

/// x86_64h（Haswell 指令集）需要 AVX2；无 AVX2 的 x86_64 CPU 上不可运行。
/// 原版/朴素 select 把 x86_64h 当首选 → 老 Intel Mac 瘦身后二进制直接崩。
#[cfg(target_arch = "x86_64")]
fn cpu_supports_x86_64h() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn cpu_supports_x86_64h() -> bool {
    false
}

/// 选择**当前 CPU 可运行**的目标切片：匹配 cputype + 能力过滤（x86_64h 仅在
/// AVX2 CPU 上可选），再取最高 subtype。arm64e 不做能力过滤 —— macOS 上
/// 所有支持 arm64 的设备（M 系/A12+）都支持指针认证。
pub fn select_runnable_slice(slices: &[FatSlice], cputype: u32) -> Option<&FatSlice> {
    slices
        .iter()
        .filter(|s| s.cputype == cputype)
        .filter(|s| s.cpusubtype != CPU_SUBTYPE_X86_64H || cpu_supports_x86_64h())
        .max_by_key(|s| s.cpusubtype)
}

/// 当前运行架构的 cputype（运行时选择，不用编译期硬编码）。
/// 不支持的架构返回 0 → select_slice 恒 None（当前平台无切片可保留）。
pub fn current_cputype() -> u32 {
    if cfg!(target_arch = "aarch64") {
        CPU_TYPE_ARM64
    } else if cfg!(target_arch = "x86_64") {
        CPU_TYPE_X86_64
    } else {
        0
    }
}

/// 架构显示名（"arm64"/"arm64e"/"x86_64"/"x86_64h"/"unknown"）
pub fn cpu_name(cputype: u32, cpusubtype: u32) -> &'static str {
    match (cputype, cpusubtype) {
        (CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E) => "arm64e",
        (CPU_TYPE_ARM64, _) => "arm64",
        (CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64H) => "x86_64h",
        (CPU_TYPE_X86_64, _) => "x86_64",
        _ => "unknown",
    }
}

/// 提取切片字节。调用方须先 parse_fat（边界已在解析时校验）。
pub fn thin_bytes(full: &[u8], slice: &FatSlice) -> Vec<u8> {
    let start = slice.offset as usize;
    full[start..start + slice.size as usize].to_vec()
}

// ---------- 扫描 ----------

/// 已知的二进制扩展名（快判白名单：不读文件即可认定候选，对齐原版 isExecutableBinary）
const BINARY_EXTENSIONS: [&str; 3] = ["dylib", "so", "bundle"];

/// 文件是否值得进一步判读：扩展名白名单，或头部是 Mach-O 家族 magic
/// （4 种 MH + 2 种 fat —— 原版 magic 列表含 fat magic，勿漏）
pub fn is_binary_candidate(file_name: &str, head: &[u8]) -> bool {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if BINARY_EXTENSIONS.contains(&ext.as_str()) {
        return true;
    }
    if head.len() < 4 {
        return false;
    }
    is_macho_magic([head[0], head[1], head[2], head[3]])
        || is_fat_magic(head[..4].try_into().unwrap())
}

/// 递归扫描目录（跳过隐藏文件/目录，对齐原版 .skipsHiddenFiles），
/// 返回全部 fat 二进制（路径 + 解析结果）。目录不可读/文件读取失败 → 跳过。
pub fn scan_dir_fat_binaries(dir: &Path) -> Vec<(std::path::PathBuf, FatFile)> {
    let mut out = Vec::new();
    scan_recursive(dir, &mut out);
    out
}

fn scan_recursive(dir: &Path, out: &mut Vec<(std::path::PathBuf, FatFile)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        // DirEntry::file_type() 不跟随符号链接（lstat 语义）：
        // 符号链接目录可能指向树内祖先 → 无限递归；统一跳过符号链接条目。
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            scan_recursive(&path, out);
        } else if file_type.is_file() {
            if let Some(fat) = read_fat_if_candidate(&path, &name) {
                out.push((path, fat));
            }
        }
    }
}

/// 候选判定 + 读入 + 解析；不是 fat / 读取失败 → None
///
/// 只读 8 字节头 + 切片表（修原版 readToEnd 全文件读入 —— fat 文件可能几百 MB，
/// 表区只有几十字节；越界校验用文件元数据长度，不需全量内容）。
fn read_fat_if_candidate(path: &Path, name: &str) -> Option<FatFile> {
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut head = [0u8; 8];
    if file.read_exact(&mut head).is_err() {
        return None;
    }
    if !is_binary_candidate(name, &head[..4]) {
        return None;
    }
    let entry_size = match u32::from_be_bytes([head[0], head[1], head[2], head[3]]) {
        FAT_MAGIC => FAT_ARCH32_SIZE,
        FAT_MAGIC_64 => FAT_ARCH64_SIZE,
        _ => return None,
    };
    let nfat = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
    if nfat == 0 || nfat > MAX_SLICES {
        return None;
    }
    let table_len = nfat as usize * entry_size;
    let mut table = vec![0u8; table_len];
    if file.read_exact(&mut table).is_err() {
        return None;
    }
    parse_table(&table, entry_size == FAT_ARCH64_SIZE, file_len).ok()
}

// ---------- 瘦身执行 ----------

/// 瘦身：把文件替换为目标切片字节（不可逆）。
/// 安全策略（修原版直接覆盖写回的问题）：读目标切片 → 写同目录临时文件（复制
/// 原权限位）→ rename 原子替换。中断不损坏原文件；rename 覆盖语义为 POSIX 行为。
pub fn thin_file(path: &Path, slice: &FatSlice) -> std::io::Result<u64> {
    // 只读目标切片字节，不整文件读入内存（修原版 readToEnd 全量读的问题）
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(slice.offset))?;
    let mut payload = vec![0u8; slice.size as usize];
    file.read_exact(&mut payload)?;

    let parent = path.parent().unwrap_or(Path::new("."));
    let tmp = parent.join(format!(
        ".apios-thin-{}-{}",
        std::process::id(),
        path.file_name().and_then(|n| n.to_str()).unwrap_or("bin")
    ));
    let perms = std::fs::metadata(path)?.permissions();

    let write_result = (|| -> std::io::Result<()> {
        let mut tmp_file = File::create(&tmp)?;
        tmp_file.set_permissions(perms)?;
        tmp_file.write_all(&payload)?;
        // fsync 后再 rename —— 保证 rename 时临时文件内容已落盘，崩溃也不会暴露半截文件
        tmp_file.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        // 临时文件残留清理（尽力而为）
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(payload.len() as u64)
}

/// 刷新目录 mtime（原版瘦身后 setAttributes 刷新 Finder 占用大小的行为）。
/// 目录在 POSIX 上不能用写模式 open（EISDIR），用只读 fd + set_times。
pub fn touch_dir(path: &Path) -> std::io::Result<()> {
    let now = std::time::SystemTime::now();
    let times = std::fs::FileTimes::new().set_modified(now);
    File::options().read(true).open(path)?.set_times(times)
}

/// ad-hoc 重签（可选增强：原版接受签名失效，`--sign` 时修复）。
/// macOS 专属（模块整体已门控）。
pub fn re_sign(path: &Path) -> Result<(), String> {
    let output = std::process::Command::new("codesign")
        .arg("-s")
        .arg("-")
        .arg(path)
        .output()
        .map_err(|e| format!("failed to run codesign: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(5).collect();
        let tail = tail.iter().rev().copied().collect::<Vec<_>>().join("\n");
        Err(format!("codesign failed:\n{tail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- fixture 构造 ----------

    fn be32(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    /// 手工构造 fat32 文件：头 + 切片表（offset/size 为 u32）+ 各切片 payload。
    /// slice payload 依次拼接，offset 由 (8 + 表长) 对齐推导 —— 真实 lipo 会按
    /// align 对齐，测试里直接顺序排布即可。
    fn make_fat32(slices: &[(u32, u32, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        bytes.extend_from_slice(&(slices.len() as u32).to_be_bytes());
        for _ in slices {
            bytes.extend_from_slice(&[0u8; FAT_ARCH32_SIZE]); // 表占位
        }
        // 回填 offset/size 并追加 payload
        let mut cursor = 8 + slices.len() * FAT_ARCH32_SIZE;
        for (i, (ct, cst, payload)) in slices.iter().enumerate() {
            let entry = 8 + i * FAT_ARCH32_SIZE;
            bytes[entry..entry + 4].copy_from_slice(&be32(*ct));
            bytes[entry + 4..entry + 8].copy_from_slice(&be32(*cst));
            bytes[entry + 8..entry + 12].copy_from_slice(&be32(cursor as u32));
            bytes[entry + 12..entry + 16].copy_from_slice(&be32(payload.len() as u32));
            bytes.extend_from_slice(payload);
            cursor += payload.len();
        }
        bytes
    }

    fn make_fat64(slices: &[(u32, u32, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC_64.to_be_bytes());
        bytes.extend_from_slice(&(slices.len() as u32).to_be_bytes());
        let table_len = slices.len() * FAT_ARCH64_SIZE;
        for _ in slices {
            bytes.extend_from_slice(&[0u8; FAT_ARCH64_SIZE]); // 表占位
        }
        let mut cursor = 8 + table_len;
        for (i, (ct, cst, payload)) in slices.iter().enumerate() {
            let entry = 8 + i * FAT_ARCH64_SIZE;
            bytes[entry..entry + 4].copy_from_slice(&be32(*ct));
            bytes[entry + 4..entry + 8].copy_from_slice(&be32(*cst));
            bytes[entry + 8..entry + 16].copy_from_slice(&(cursor as u64).to_be_bytes());
            bytes[entry + 16..entry + 24].copy_from_slice(&(payload.len() as u64).to_be_bytes());
            bytes.extend_from_slice(payload);
            cursor += payload.len();
        }
        bytes
    }

    fn slice(ct: u32, cst: u32, off: u64, size: u64) -> FatSlice {
        FatSlice {
            cputype: ct,
            cpusubtype: cst,
            offset: off,
            size,
        }
    }

    // ---------- 解析 ----------

    #[test]
    fn test_parse_fat32_two_slices() {
        let payload_arm = [0xAAu8; 64];
        let payload_x86 = [0xBBu8; 32];
        let fat = make_fat32(&[
            (CPU_TYPE_ARM64, 0, &payload_arm),
            (CPU_TYPE_X86_64, 0, &payload_x86),
        ]);
        let parsed = parse_fat(&fat).unwrap();
        assert_eq!(parsed.slices.len(), 2);
        assert_eq!(
            parsed.slices[0],
            slice(CPU_TYPE_ARM64, 0, 48, 64) // 8 + 2*20 = 48
        );
        assert_eq!(parsed.slices[1], slice(CPU_TYPE_X86_64, 0, 48 + 64, 32));
    }

    #[test]
    fn test_parse_fat64_large_offsets() {
        let payload = [0xCCu8; 16];
        let fat = make_fat64(&[
            (CPU_TYPE_ARM64, 0, &payload),
            (CPU_TYPE_X86_64, 0, &[0xDD; 8]),
        ]);
        let parsed = parse_fat(&fat).unwrap();
        assert_eq!(parsed.slices.len(), 2);
        // fat64 表 8 + 2*32 = 72
        assert_eq!(parsed.slices[0], slice(CPU_TYPE_ARM64, 0, 72, 16));
        assert_eq!(parsed.slices[1], slice(CPU_TYPE_X86_64, 0, 72 + 16, 8));
    }

    #[test]
    fn test_parse_fat64_overflow_guard() {
        // offset + size 溢出 u64（最高位切片）→ Invalid 而非 panic
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC_64.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        let mut entry = [0u8; 32];
        entry[0..4].copy_from_slice(&be32(CPU_TYPE_ARM64));
        entry[8..16].copy_from_slice(&u64::MAX.to_be_bytes());
        entry[16..24].copy_from_slice(&1u64.to_be_bytes());
        bytes.extend_from_slice(&entry);
        assert_eq!(
            parse_fat(&bytes),
            Err(LipError::Invalid("slice size overflow"))
        );
    }

    #[test]
    fn test_parse_fat32_slice_out_of_bounds() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&be32(CPU_TYPE_ARM64));
        bytes.extend_from_slice(&be32(0));
        bytes.extend_from_slice(&be32(8 + 20)); // 表外
        bytes.extend_from_slice(&be32(9999)); // 越界
        bytes.extend_from_slice(&be32(0));
        assert_eq!(
            parse_fat(&bytes),
            Err(LipError::Invalid("slice out of bounds"))
        );
    }

    #[test]
    fn test_parse_fat_nfat_zero_and_too_many() {
        let mut zero = Vec::new();
        zero.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        zero.extend_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            parse_fat(&zero),
            Err(LipError::Invalid("nfat_arch is zero"))
        );

        let mut many = Vec::new();
        many.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        many.extend_from_slice(&(MAX_SLICES + 1).to_be_bytes());
        assert_eq!(
            parse_fat(&many),
            Err(LipError::Invalid("nfat_arch exceeds sane limit"))
        );
    }

    #[test]
    fn test_parse_fat_truncated() {
        assert_eq!(parse_fat(&[]), Err(LipError::Truncated("header")));
        assert_eq!(
            parse_fat(&[0xca, 0xfe, 0xba, 0xbe]),
            Err(LipError::Truncated("header"))
        );

        // nfat=2 但表只有 1 条
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[0u8; FAT_ARCH32_SIZE]);
        assert_eq!(
            parse_fat(&bytes),
            Err(LipError::Truncated("architecture table"))
        );
    }

    #[test]
    fn test_parse_fat_not_fat() {
        let thin = [0xcf, 0xfa, 0xed, 0xfe, 0x00, 0x00, 0x00, 0x00]; // MH_MAGIC_64
        assert_eq!(parse_fat(&thin), Err(LipError::NotFat));
    }

    #[test]
    fn test_parse_fat_zero_size_slice() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FAT_MAGIC.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&be32(CPU_TYPE_ARM64));
        bytes.extend_from_slice(&be32(0));
        bytes.extend_from_slice(&be32(28));
        bytes.extend_from_slice(&be32(0)); // size 0
        bytes.extend_from_slice(&be32(0));
        assert_eq!(
            parse_fat(&bytes),
            Err(LipError::Invalid("slice size is zero"))
        );
    }

    // ---------- 切片选择 ----------

    #[test]
    fn test_select_slice_matches_cputype() {
        let fat = FatFile {
            slices: vec![
                slice(CPU_TYPE_ARM64, 0, 0, 10),
                slice(CPU_TYPE_X86_64, 0, 0, 20),
            ],
        };
        assert_eq!(select_slice(&fat.slices, CPU_TYPE_ARM64).unwrap().size, 10);
        assert_eq!(select_slice(&fat.slices, CPU_TYPE_X86_64).unwrap().size, 20);
    }

    #[test]
    fn test_select_slice_prefers_highest_subtype() {
        // arm64 + arm64e 同 cputype → 取 arm64e（修原版取第一个的问题）
        let fat = FatFile {
            slices: vec![
                slice(CPU_TYPE_ARM64, 0, 0, 10),
                slice(CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E, 0, 30),
            ],
        };
        assert_eq!(select_slice(&fat.slices, CPU_TYPE_ARM64).unwrap().size, 30);
        // x86_64 + x86_64h
        let fat = FatFile {
            slices: vec![
                slice(CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64H, 0, 40),
                slice(CPU_TYPE_X86_64, 0, 0, 20),
            ],
        };
        assert_eq!(select_slice(&fat.slices, CPU_TYPE_X86_64).unwrap().size, 40);
    }

    #[test]
    fn test_select_slice_no_match() {
        let fat = FatFile {
            slices: vec![slice(CPU_TYPE_ARM64, 0, 0, 10)],
        };
        assert!(select_slice(&fat.slices, CPU_TYPE_X86_64).is_none());
    }

    /// 生产选择：x86_64h 仅在有 AVX2 的 CPU 上可选（无则降级普通 x86_64）
    #[test]
    fn test_select_runnable_x86_64h_capability_gate() {
        let fat = FatFile {
            slices: vec![
                slice(CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64H, 0, 40),
                slice(CPU_TYPE_X86_64, 0, 0, 20),
            ],
        };
        let picked = select_runnable_slice(&fat.slices, CPU_TYPE_X86_64).unwrap();
        if cpu_supports_x86_64h() {
            assert_eq!(picked.size, 40, "AVX2 CPU → 取 x86_64h");
        } else {
            assert_eq!(picked.size, 20, "无 AVX2 → 降级普通 x86_64");
        }
    }

    /// 能力过滤不影响 arm64e 优先（所有 arm64 平台都支持指针认证）
    #[test]
    fn test_select_runnable_arm64e_still_preferred() {
        let fat = FatFile {
            slices: vec![
                slice(CPU_TYPE_ARM64, 0, 0, 10),
                slice(CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E, 0, 30),
            ],
        };
        let picked = select_runnable_slice(&fat.slices, CPU_TYPE_ARM64).unwrap();
        assert_eq!(picked.size, 30);
    }

    #[test]
    fn test_current_cputype_matches_target() {
        // 当前编译架构恒有匹配切片（select_slice 与 current_cputype 联动）
        let expected = if cfg!(target_arch = "aarch64") {
            CPU_TYPE_ARM64
        } else if cfg!(target_arch = "x86_64") {
            CPU_TYPE_X86_64
        } else {
            0
        };
        assert_eq!(current_cputype(), expected);
    }

    // ---------- 提取 ----------

    #[test]
    fn test_thin_bytes_extracts_exact_slice() {
        let payload_arm = [0xAAu8; 64];
        let payload_x86 = [0xBBu8; 32];
        let fat = make_fat32(&[
            (CPU_TYPE_ARM64, 0, &payload_arm),
            (CPU_TYPE_X86_64, 0, &payload_x86),
        ]);
        let parsed = parse_fat(&fat).unwrap();
        let arm = select_slice(&parsed.slices, CPU_TYPE_ARM64).unwrap();
        assert_eq!(thin_bytes(&fat, arm), payload_arm);
        let x86 = select_slice(&parsed.slices, CPU_TYPE_X86_64).unwrap();
        assert_eq!(thin_bytes(&fat, x86), payload_x86);
    }

    // ---------- magic / 候选判定 ----------

    #[test]
    fn test_magic_detection() {
        assert!(is_fat_magic(FAT_MAGIC.to_be_bytes()));
        assert!(is_fat_magic(FAT_MAGIC_64.to_be_bytes()));
        assert!(!is_fat_magic(MH_MAGIC.to_be_bytes()));
        assert!(is_macho_magic(MH_MAGIC.to_be_bytes()));
        assert!(is_macho_magic(MH_CIGAM.to_be_bytes()));
        assert!(is_macho_magic(MH_MAGIC_64.to_be_bytes()));
        assert!(is_macho_magic(MH_CIGAM_64.to_be_bytes()));
        assert!(!is_macho_magic(FAT_MAGIC.to_be_bytes()));
        assert!(!is_macho_magic(0x1122_3344u32.to_be_bytes()));
    }

    #[test]
    fn test_binary_candidate_extensions() {
        // 白名单扩展名：不读文件即认定（head 任意/空）
        assert!(is_binary_candidate("libfoo.dylib", &[]));
        assert!(is_binary_candidate("foo.so", &[]));
        assert!(is_binary_candidate("x.bundle", &[]));
        assert!(is_binary_candidate("libfoo.DYLIB", &[])); // 大小写不敏感
    }

    #[test]
    fn test_binary_candidate_magic() {
        // 无扩展名文件：靠 magic（fat 与 thin 都算候选）
        assert!(is_binary_candidate("Foo", &FAT_MAGIC.to_be_bytes()));
        assert!(is_binary_candidate("Foo", &MH_MAGIC_64.to_be_bytes()));
        // 非候选
        assert!(!is_binary_candidate(
            "readme.txt",
            &[0x11, 0x22, 0x33, 0x44]
        ));
        assert!(!is_binary_candidate("Foo", &[])); // 无 magic 可判
    }

    #[test]
    fn test_cpu_names() {
        assert_eq!(cpu_name(CPU_TYPE_ARM64, 0), "arm64");
        assert_eq!(cpu_name(CPU_TYPE_ARM64, CPU_SUBTYPE_ARM64E), "arm64e");
        assert_eq!(cpu_name(CPU_TYPE_X86_64, 0), "x86_64");
        assert_eq!(cpu_name(CPU_TYPE_X86_64, CPU_SUBTYPE_X86_64H), "x86_64h");
        assert_eq!(cpu_name(0x1234, 0), "unknown");
    }

    // ---------- 文件系统（tempdir） ----------

    #[test]
    fn test_scan_dir_fat_binaries_finds_fat_only() {
        let tmp = tempfile::TempDir::new().unwrap();
        // 真实 fat 文件（名字无扩展名）
        let fat_bytes = make_fat32(&[
            (CPU_TYPE_ARM64, 0, &[0xAA; 32]),
            (CPU_TYPE_X86_64, 0, &[0xBB; 16]),
        ]);
        std::fs::write(tmp.path().join("tool"), &fat_bytes).unwrap();
        // 子目录里的 fat dylib
        std::fs::create_dir(tmp.path().join("lib")).unwrap();
        std::fs::write(tmp.path().join("lib/libx.dylib"), &fat_bytes).unwrap();
        // 非 fat：thin Mach-O 名字无扩展名、文本文件、隐藏文件（跳过）
        std::fs::write(
            tmp.path().join("thin"),
            [0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0],
        )
        .unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join(".hidden-fat"), &fat_bytes).unwrap();

        let found = scan_dir_fat_binaries(tmp.path());
        let mut names: Vec<String> = found
            .iter()
            .map(|(p, _)| p.strip_prefix(tmp.path()).unwrap().display().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["lib/libx.dylib", "tool"]);
    }

    /// 符号链接目录（指向树内祖先 → 成环）不得导致无限递归
    #[cfg(unix)]
    #[test]
    fn test_scan_skips_symlink_dirs_no_cycle() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::os::unix::fs::symlink(tmp.path(), sub.join("loop")).unwrap();
        std::fs::write(
            tmp.path().join("tool"),
            make_fat32(&[(CPU_TYPE_ARM64, 0, &[0xAA; 16])]),
        )
        .unwrap();
        let found = scan_dir_fat_binaries(tmp.path());
        assert_eq!(found.len(), 1, "符号链接目录被跳过，扫描正常结束");
    }

    #[test]
    fn test_thin_file_replaces_with_slice() {
        let tmp = tempfile::TempDir::new().unwrap();
        let payload_arm = [0xAAu8; 32];
        let payload_x86 = [0xBBu8; 16];
        let fat_bytes = make_fat32(&[
            (CPU_TYPE_ARM64, 0, &payload_arm),
            (CPU_TYPE_X86_64, 0, &payload_x86),
        ]);
        let path = tmp.path().join("tool");
        std::fs::write(&path, &fat_bytes).unwrap();

        let parsed = parse_fat(&fat_bytes).unwrap();
        let arm = select_slice(&parsed.slices, CPU_TYPE_ARM64).unwrap();
        let written = thin_file(&path, arm).unwrap();
        assert_eq!(written, payload_arm.len() as u64);

        let after = std::fs::read(&path).unwrap();
        assert_eq!(after, payload_arm); // 文件 == 目标切片
        assert!(std::fs::read_dir(tmp.path()).unwrap().count() == 1); // 无临时文件残留
    }

    #[cfg(unix)]
    #[test]
    fn test_thin_file_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let payload_arm = [0xAAu8; 32];
        let payload_x86 = [0xBBu8; 16];
        let fat_bytes = make_fat32(&[
            (CPU_TYPE_ARM64, 0, &payload_arm),
            (CPU_TYPE_X86_64, 0, &payload_x86),
        ]);
        let path = tmp.path().join("tool");
        std::fs::write(&path, &fat_bytes).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();

        let parsed = parse_fat(&fat_bytes).unwrap();
        let arm = select_slice(&parsed.slices, CPU_TYPE_ARM64).unwrap();
        thin_file(&path, arm).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn test_touch_dir_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(touch_dir(tmp.path()).is_ok());
    }
}
