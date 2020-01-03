use super::Host;

// FIXME: These should both be `u16`, and probably generated from the wasi
// snapshot_0 `witx` definitions.
const __WASI_ESUCCESS: u16 = 0;
const __WASI_EIO: u16 = 29;  // Not sure I counted right. Might be some other error.

const __WASI_STDOUT: u32 = 1;

fn get_element_ptr(base: u32, offset: u32, scale: u32) -> Result<u32, wasmi::Trap> {
    let byte_offset = offset.checked_mul(scale)
        .ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?;
    Ok(base.checked_add(byte_offset).ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?)
}

/// Loads the value stored at `base + (offset * scale)`.
fn mem_read<T: wasmi::LittleEndianConvert>(
    mem: &[u8],
    base: u32,
    offset: u32,
    scale: u32,
) -> Result<T, wasmi::Trap> {
    let addr = get_element_ptr(base, offset, scale)?;
    let slice = mem.get(addr as usize..)
        .ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?;
    Ok(T::from_little_endian(slice).map_err(|_| wasmi::TrapKind::MemoryAccessOutOfBounds)?)
}

/// Stores a value at `base + (offset * scale)`.
fn mem_write<T: wasmi::LittleEndianConvert>(
    mem: &mut [u8],
    value: T,
    base: u32,
    offset: u32,
    scale: u32,
) -> Result<(), wasmi::Trap> {
    let addr = get_element_ptr(base, offset, scale)?;

    // NOTE: while `from_little_endian` will trim the input slice to be the
    // correct length, `into_little_endian` does not, and requires that `slice`
    // be the correct length for our data type.
    //
    // Assume that the size in wasm matches our host size for the given type.
    let addr_after = get_element_ptr(addr, core::mem::size_of::<T>() as u32, 1)?;
    let slice = mem.get_mut(addr as usize..addr_after as usize)
        .ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?;
    Ok(T::into_little_endian(value, slice))
}

/// Reference to a subslice of memory.
fn mem_slice(mem: &[u8], addr: u32, len: u32) -> Result<&[u8], wasmi::Trap> {
    let end = get_element_ptr(addr, len, 1)?;
    Ok(mem.get(addr as usize..end as usize).ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?)
}

#[allow(dead_code)] // unused
fn mem_slice_mut(mem: &mut [u8], addr: u32, len: u32) -> Result<&mut [u8], wasmi::Trap> {
    let end = get_element_ptr(addr, len, 1)?;
    Ok(mem.get_mut(addr as usize..end as usize).ok_or(wasmi::TrapKind::MemoryAccessOutOfBounds)?)
}

#[tracing::instrument(skip(host))]
pub fn fd_write(host: &mut Host, fd: u32, iovs: u32, iovs_len: u32, nwritten: u32) -> Result<u16, wasmi::Trap> {
    if fd != __WASI_STDOUT {
        return Ok(__WASI_EIO);
    }

    host.memory.with_direct_access_mut(|mem| {
        let mut bytes_written = 0u32;
        for idx in 0..iovs_len {
            // read iov
            let iov = get_element_ptr(iovs, 8, idx)?;
            let buf = mem_read::<u32>(mem, iov, 0, 4)?;
            let buf_len = mem_read::<u32>(mem, iov, 1, 4)?;

            // TODO: Actually write out our slice.
            let buf_slice = mem_slice(mem, buf, buf_len)?;
            let buf_str = core::str::from_utf8(buf_slice);
            tracing::info!(?buf_slice, ?buf_str, "fd_write");

            bytes_written = bytes_written.saturating_add(buf_len);
        }

        // Write number of bytes written to memory.
        mem_write::<u32>(mem, bytes_written, nwritten, 0, 0)?;
        Ok(__WASI_ESUCCESS)
    })
}
