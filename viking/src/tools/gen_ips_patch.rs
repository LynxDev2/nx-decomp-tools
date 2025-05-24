use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;

use viking::*;

static IPS_HEADER_MAGIC: &'static [u8; 5] = b"IPS32";
static IPS_EOF_MAGIC: &'static [u8; 4] = b"EEOF";
static NSO_HEADER_LEN: u32 = 0x100;

static SMO_BID: &'static str = "3CA12DFAAF9C82DA064D1698DF79CDA1";

// Offset of subsdk1 start relative to main, same as end of subsdk0
static SMO_SUBSDK1_START: u32 = 0x2C63000;

fn main() -> Result<()> {
    let decomp_elf = elf::load_decomp_elf(None).context("Decomp elf not found")?;
    let file_list = functions::parse_file_list(functions::get_file_list_path(None).as_path())?;
    let functions = functions::get_functions(&file_list);

    let mut ips_file = File::create("out.ips")?;

    ips_file.write_all(IPS_HEADER_MAGIC)?;

    for function in functions {
        if function.status != functions::Status::Matching {
            continue;
        }
        if let Ok(sym) = elf::find_function_symbol_by_name(&decomp_elf, &function.name()) {
            ips_file.write_all(&create_branch_patch(
                function.offset as u32,
                sym.st_value as u32,
            ))?;
        }
    }

    ips_file.write_all(IPS_EOF_MAGIC)?;

    Ok(())
}

// Patch format: offset (4 bytes), patch size (2 bytes), patch data (4 bytes for this use case)
fn create_branch_patch(original_offset: u32, target_offset: u32) -> [u8; 10] {
    let mut patch_data = [0u8; 10];
    let patch_offset = original_offset + NSO_HEADER_LEN;
    patch_data[0..4].copy_from_slice(&patch_offset.to_be_bytes());
    // Set patch size to 4
    patch_data[5] = 4;

    let function_loaded_at = SMO_SUBSDK1_START + target_offset;
    patch_data[6..].copy_from_slice(&assemble_branch_instruction_arm64_le(
        function_loaded_at.wrapping_sub(original_offset),
    ));

    patch_data
}

fn assemble_branch_instruction_arm64_le(offset_rel_bytes: u32) -> [u8; 4] {
    let offset_instr = (offset_rel_bytes as i32) / 4;

    if offset_instr < -(1 << 25) || offset_instr >= (1 << 25) {
        panic!("Offset out of range for B instruction");
    }

    let opcode: u32 = 0b000101 << 26;
    let encoded: u32 = opcode | ((offset_instr as u32) & 0x03FF_FFFF);

    encoded.to_le_bytes()
}
