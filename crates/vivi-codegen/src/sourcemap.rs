/// Source mapping entry: maps a WASM code offset to a source location.
#[derive(Debug, Clone)]
pub struct SourceMapping {
    /// Byte offset within the WASM module (absolute, from start of file)
    pub wasm_offset: u32,
    /// 0-based source line
    pub source_line: u32,
    /// 0-based source column
    pub source_col: u32,
}

/// Raw mapping collected during codegen (relative to function body).
#[derive(Debug, Clone)]
pub struct RawMapping {
    /// Index of instructions emitted so far when this mapping was recorded
    pub instr_index: u32,
    /// 0-based source line
    pub source_line: u32,
    /// 0-based source column
    pub source_col: u32,
}

/// Per-function mapping data collected during codegen.
#[derive(Debug, Clone, Default)]
pub struct FuncMappings {
    pub entries: Vec<RawMapping>,
}

/// All mappings for the entire module.
#[derive(Debug, Clone, Default)]
pub struct ModuleMappings {
    /// func_index (in code section order, 0-based among local funcs) → mappings
    pub functions: Vec<FuncMappings>,
}

/// VLQ encode a signed integer.
fn vlq_encode(value: i32) -> String {
    const VLQ_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut v = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    } as u32;

    let mut result = String::new();
    loop {
        let mut digit = v & 0x1F;
        v >>= 5;
        if v > 0 {
            digit |= 0x20; // continuation bit
        }
        result.push(VLQ_CHARS[digit as usize] as char);
        if v == 0 {
            break;
        }
    }
    result
}

/// Generate a Source Map v3 JSON string.
///
/// For WASM source maps, each "line" in the generated file corresponds to a
/// WASM byte offset. The mappings use a special format where each mapping
/// segment encodes (column=wasm_offset, source_idx, source_line, source_col).
pub fn generate_source_map(
    source_filename: &str,
    source_content: &str,
    mappings: &[SourceMapping],
) -> String {
    // WASM source maps use a single "line" (line 0) with column = wasm byte offset
    // Each segment: [generated_col, source_idx(0), source_line, source_col]
    let mut segments: Vec<String> = Vec::new();
    let mut prev_wasm_offset: i32 = 0;
    let mut prev_source_line: i32 = 0;
    let mut prev_source_col: i32 = 0;

    for m in mappings {
        let mut seg = String::new();
        seg.push_str(&vlq_encode(m.wasm_offset as i32 - prev_wasm_offset));
        seg.push_str(&vlq_encode(0)); // source index (always 0, single file)
        seg.push_str(&vlq_encode(m.source_line as i32 - prev_source_line));
        seg.push_str(&vlq_encode(m.source_col as i32 - prev_source_col));

        prev_wasm_offset = m.wasm_offset as i32;
        prev_source_line = m.source_line as i32;
        prev_source_col = m.source_col as i32;

        segments.push(seg);
    }

    let mappings_str = segments.join(",");

    // Escape source content for JSON
    let escaped_content = source_content
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");

    format!(
        r#"{{"version":3,"sources":["{source_filename}"],"sourcesContent":["{escaped_content}"],"mappings":"{mappings_str}"}}"#
    )
}

/// Scan a WASM binary to find function body byte offsets within the code section.
/// Returns a list of (absolute_offset_of_func_body_start, body_size) for each
/// local function, in order.
pub fn find_func_body_offsets(wasm_bytes: &[u8]) -> Vec<(u32, u32)> {
    let mut pos = 8; // skip magic + version
    let mut results = Vec::new();

    while pos < wasm_bytes.len() {
        let section_id = wasm_bytes[pos];
        pos += 1;
        let (section_size, bytes_read) = read_leb128_u32(&wasm_bytes[pos..]);
        pos += bytes_read;
        let section_end = pos + section_size as usize;

        if section_id == 10 {
            // Code section
            let (func_count, br) = read_leb128_u32(&wasm_bytes[pos..]);
            pos += br;

            for _ in 0..func_count {
                let (body_size, br) = read_leb128_u32(&wasm_bytes[pos..]);
                pos += br;
                let body_start = pos as u32;
                results.push((body_start, body_size));
                pos += body_size as usize;
            }
            break;
        } else {
            pos = section_end;
        }
    }

    results
}

fn read_leb128_u32(bytes: &[u8]) -> (u32, usize) {
    let mut result: u32 = 0;
    let mut shift = 0;
    let mut pos = 0;
    loop {
        let byte = bytes[pos];
        result |= ((byte & 0x7F) as u32) << shift;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    (result, pos)
}

/// Resolve raw per-function mappings into absolute WASM byte offset mappings.
///
/// This scans the WASM binary to find where each function body starts,
/// then uses a simple heuristic to estimate instruction byte offsets within
/// each function body.
pub fn resolve_mappings(
    wasm_bytes: &[u8],
    module_mappings: &ModuleMappings,
) -> Vec<SourceMapping> {
    let func_offsets = find_func_body_offsets(wasm_bytes);
    let mut result = Vec::new();

    for (func_idx, func_map) in module_mappings.functions.iter().enumerate() {
        if func_idx >= func_offsets.len() || func_map.entries.is_empty() {
            continue;
        }
        let (body_start, body_size) = func_offsets[func_idx];

        // Skip the local declarations in the function body to find code start
        let mut code_start = body_start as usize;
        let (local_count, br) = read_leb128_u32(&wasm_bytes[code_start..]);
        code_start += br;
        for _ in 0..local_count {
            let (_count, br) = read_leb128_u32(&wasm_bytes[code_start..]);
            code_start += br;
            code_start += 1; // valtype byte
        }

        let code_len = (body_start as usize + body_size as usize) - code_start;
        let total_instrs = func_map.entries.last().map_or(1, |e| e.instr_index + 1) as usize;

        for entry in &func_map.entries {
            // Estimate byte offset: distribute evenly across the code bytes
            let estimated_offset = if total_instrs > 0 {
                code_start + (entry.instr_index as usize * code_len / total_instrs.max(1))
            } else {
                code_start
            };

            result.push(SourceMapping {
                wasm_offset: estimated_offset as u32,
                source_line: entry.source_line,
                source_col: entry.source_col,
            });
        }
    }

    result.sort_by_key(|m| m.wasm_offset);
    // Deduplicate by wasm_offset
    result.dedup_by_key(|m| m.wasm_offset);
    result
}

/// Create the sourceMappingURL custom section bytes.
pub fn source_mapping_url_section(url: &str) -> Vec<u8> {
    let section_name = b"sourceMappingURL";
    let name_len = section_name.len();
    let url_bytes = url.as_bytes();
    let url_len = url_bytes.len();

    let content_size = leb128_size(name_len as u32) + name_len + url_len;
    let mut data = Vec::new();
    data.push(0); // custom section id
    write_leb128_u32(&mut data, content_size as u32);
    write_leb128_u32(&mut data, name_len as u32);
    data.extend_from_slice(section_name);
    data.extend_from_slice(url_bytes);
    data
}

fn leb128_size(mut value: u32) -> usize {
    let mut size = 0;
    loop {
        size += 1;
        value >>= 7;
        if value == 0 { break; }
    }
    size
}

fn write_leb128_u32(buf: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value > 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 { break; }
    }
}
