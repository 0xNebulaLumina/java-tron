// Protobuf parsing utilities
// Shared protobuf decoding helpers

/// Read a protobuf varint from a byte slice
/// Returns (value, bytes_read)
pub(crate) fn read_varint(data: &[u8]) -> Result<(u64, usize), String> {
    let mut result: u64 = 0;
    let mut shift = 0;
    let mut pos = 0;

    loop {
        if pos >= data.len() {
            return Err("Unexpected end of varint".to_string());
        }

        let byte = data[pos];
        pos += 1;

        result |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            return Ok((result, pos));
        }

        shift += 7;
        if shift >= 64 {
            return Err("Varint too long".to_string());
        }
    }
}