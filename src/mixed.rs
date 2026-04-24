use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyByteArray, PyList, PyTuple};

use crate::error::ShmError;

/// Converts a Python `list[("type", list[...])]` into a flat `Vec<u8>`.
///
/// Supported type tags:
///
/// | Tag      | Rust type | Bytes per element |
/// |----------|-----------|-------------------|
/// | `"f32"`  | `f32`     | 4 |
/// | `"f64"`  | `f64`     | 8 |
/// | `"i32"`  | `i32`     | 4 |
/// | `"i64"`  | `i64`     | 8 |
/// | `"u8"`   | `u8`      | 1 |
/// | `"u32"`  | `u32`     | 4 |
/// | `"u64"`  | `u64`     | 8 |
/// | `"bool"` | `u8` (0/1)| 1 |
///
/// # Example (Python)
///
/// ```python
/// buf = view.pack_mixed([
///     ("f32",  [1.0, 2.5]),
///     ("i64",  [100, 200]),
///     ("bool", [True, False]),
/// ])
/// # buf is bytes; total size = 2*4 + 2*8 + 2*1 = 26 bytes
/// ```
/// Unpacks a flat byte slice back into a Python list of typed values.
///
/// `schema` is a Python `list` of `(type_tag, count)` tuples that describes
/// the layout — the same tags used in [`pack_mixed`].
///
/// Returns a `list` of `list`, one inner list per schema entry.
///
/// # Example (Python)
///
/// ```python
/// result = unpack_mixed(raw_bytes, [
///     ("f32",   2),   # → [1.0, 2.5]
///     ("i64",   2),   # → [100, 200]
///     ("bool",  2),   # → [True, False]
///     ("str16", 3),   # → ["Normal", "DDoS", "DoS"]
/// ])
/// ```
pub fn unpack_mixed<'py>(
    py: Python<'py>,
    data: &[u8],
    schema: &Bound<'py, PyList>,
) -> PyResult<Bound<'py, PyList>> {
    let result = PyList::empty(py);
    let mut cursor = 0usize;

    for item in schema.iter() {
        let tuple = item.cast::<PyTuple>().map_err(|_| {
            ShmError::InvalidArg("schema entries must be (type_tag, count) tuples".into())
        })?;

        let tag: String = tuple.get_item(0)?.extract()?;
        let count = tuple.get_item(1)?.extract::<usize>()?;
        let group = PyList::empty(py);

        macro_rules! read_fixed {
            ($size:expr, $t:ty) => {{
                for _ in 0..count {
                    check_bounds(cursor, $size, data.len())?;
                    let v = <$t>::from_ne_bytes(
                        data[cursor..cursor + $size].try_into().unwrap()
                    );
                    group.append(v)?;
                    cursor += $size;
                }
            }};
        }

        match tag.as_str() {
            "bool" => {
                for _ in 0..count {
                    check_bounds(cursor, 1, data.len())?;
                    group.append(data[cursor] != 0)?;
                    cursor += 1;
                }
            }
            "int"   | "i64" => read_fixed!(8, i64),
            "float" | "f64" => read_fixed!(8, f64),
            "f32"  => read_fixed!(4, f32),
            "i32"  => read_fixed!(4, i32),
            "u8"   => {
                for _ in 0..count {
                    check_bounds(cursor, 1, data.len())?;
                    group.append(data[cursor])?;
                    cursor += 1;
                }
            }
            "u32"  => read_fixed!(4, u32),
            "u64"  => read_fixed!(8, u64),
            "str"  => {
                // Length-prefixed UTF-8
                for _ in 0..count {
                    check_bounds(cursor, 4, data.len())?;
                    let len = u32::from_ne_bytes(
                        data[cursor..cursor+4].try_into().unwrap()
                    ) as usize;
                    cursor += 4;
                    check_bounds(cursor, len, data.len())?;
                    let s = String::from_utf8_lossy(&data[cursor..cursor+len]);
                    group.append(s.as_ref())?;
                    cursor += len;
                }
            }
            other => {
                return Err(ShmError::InvalidArg(format!(
                    "unknown type tag '{other}'; supported: bool, int, float, f32, f64, i32, i64, u8, u32, u64, str"
                )).into());
            }
        }

        if count == 1 {
            result.append(group.get_item(0)?)?;
        } else {
            result.append(&group)?;
        }
    }

    Ok(result)
}

fn check_bounds(cursor: usize, needed: usize, total: usize) -> PyResult<()> {
    if cursor + needed > total {
        Err(ShmError::InvalidArg(format!(
            "out of bounds: cursor={cursor} + needed={needed} > data_len={total}"
        )).into())
    } else {
        Ok(())
    }
}

/// Packs a single Python value into bytes by inferring the type automatically.
///
/// | Python type | Stored as | Bytes |
/// |-------------|-----------|-------|
/// | `bool`      | `u8` (0/1) | 1 |
/// | `int`       | `i64`      | 8 |
/// | `float`     | `f64`      | 8 |
/// | `str`       | `u32` length prefix + UTF-8 bytes | 4 + len |
/// | `bytes`     | raw bytes  | len |
/// | `bytearray` | raw bytes  | len |
pub fn pack_value(v: &Bound<'_, PyAny>, out: &mut Vec<u8>) -> PyResult<()> {
    use pyo3::types::{PyBool, PyInt, PyFloat, PyString};

    if v.cast::<PyBool>().is_ok() {
        // Must check bool before int (bool is a subclass of int).
        out.push(v.extract::<bool>()? as u8);
    } else if v.cast::<PyInt>().is_ok() {
        out.extend_from_slice(&v.extract::<i64>()?.to_ne_bytes());
    } else if v.cast::<PyFloat>().is_ok() {
        out.extend_from_slice(&v.extract::<f64>()?.to_ne_bytes());
    } else if let Ok(s) = v.cast::<PyString>() {
        // Length-prefixed UTF-8: [u32 len][bytes...]
        let encoded = s.to_str()?.as_bytes().to_vec();
        out.extend_from_slice(&(encoded.len() as u32).to_ne_bytes());
        out.extend_from_slice(&encoded);
    } else if let Ok(b) = v.cast::<PyBytes>() {
        out.extend_from_slice(b.as_bytes());
    } else if let Ok(ba) = v.cast::<PyByteArray>() {
        out.extend_from_slice(&ba.to_vec());
    } else {
        return Err(ShmError::InvalidArg(format!(
            "unsupported type '{}'; supported: bool, int, float, str, bytes, bytearray",
            v.get_type().name()?
        )).into());
    }
    Ok(())
}

/// Packs a heterogeneous Python list into a flat byte buffer.
///
/// Each element can be:
/// - A plain value (`int`, `float`, `bool`, `str`, `bytes`) — packed directly.
/// - A nested `list` — each element is packed in order.
///
/// No type tags needed; types are inferred automatically from the Python values.
///
/// # Example (Python)
///
/// ```python
/// buf = pack_mixed([
///     ["Normal", "DDoS", "DoS"],   # str (length-prefixed)
///     [0.92, 0.87, 0.95],          # float → f64
///     [True, False, True],         # bool → u8
///     42,                          # int → i64
/// ])
/// ```
pub fn pack_mixed(items: &Bound<'_, PyList>) -> PyResult<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();

    for item in items.iter() {
        if let Ok(sub) = item.cast::<PyList>() {
            for v in sub.iter() {
                pack_value(&v, &mut out)?;
            }
        } else {
            pack_value(&item, &mut out)?;
        }
    }

    Ok(out)
}

/// Reads one value from a byte slice according to `tag`.
///
/// Returns `(value, bytes_consumed)`.
pub fn unpack_one(py: Python<'_>, data: &[u8], tag: &str) -> PyResult<(Py<PyAny>, usize)> {
    macro_rules! read_fixed {
        ($size:expr, $t:ty) => {{
            if data.len() < $size {
                return Err(ShmError::InvalidArg(format!(
                    "not enough data to read '{tag}': need {}, have {}",
                    $size, data.len()
                )).into());
            }
            let v = <$t>::from_ne_bytes(data[..$size].try_into().unwrap());
            (v.into_pyobject(py)?.into_any().unbind(), $size)
        }};
    }

    let result = match tag {
        "bool" => {
            if data.is_empty() {
                return Err(ShmError::InvalidArg("not enough data to read 'bool'".into()).into());
            }
            (pyo3::types::PyBool::new(py, data[0] != 0).as_any().clone().unbind(), 1)
        }
        "u8"            => read_fixed!(1, u8),
        "f32" | "i32" | "u32" => {
            if data.len() < 4 {
                return Err(ShmError::InvalidArg(format!(
                    "not enough data to read '{tag}': need 4, have {}", data.len()
                )).into());
            }
            match tag {
                "f32" => {
                    let v = f32::from_ne_bytes(data[..4].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 4)
                }
                "i32" => {
                    let v = i32::from_ne_bytes(data[..4].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 4)
                }
                _ => {
                    let v = u32::from_ne_bytes(data[..4].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 4)
                }
            }
        }
        "int" | "float" | "i64" | "f64" | "u64" => {
            if data.len() < 8 {
                return Err(ShmError::InvalidArg(format!(
                    "not enough data to read '{tag}': need 8, have {}", data.len()
                )).into());
            }
            match tag {
                "float" | "f64" => {
                    let v = f64::from_ne_bytes(data[..8].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 8)
                }
                "u64" => {
                    let v = u64::from_ne_bytes(data[..8].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 8)
                }
                _ => {
                    let v = i64::from_ne_bytes(data[..8].try_into().unwrap());
                    (v.into_pyobject(py)?.into_any().clone().unbind(), 8)
                }
            }
        }
        "str" => {
            if data.len() < 4 {
                return Err(ShmError::InvalidArg("not enough data to read str length prefix".into()).into());
            }
            let len = u32::from_ne_bytes(data[..4].try_into().unwrap()) as usize;
            if data.len() < 4 + len {
                return Err(ShmError::InvalidArg(format!(
                    "not enough data to read str payload: need {}, have {}", 4 + len, data.len()
                )).into());
            }
            let s = String::from_utf8_lossy(&data[4..4 + len]).into_owned();
            (s.into_pyobject(py)?.into_any().clone().unbind(), 4 + len)
        }
        other => {
            return Err(ShmError::InvalidArg(format!(
                "unknown type tag '{other}'; supported: bool, int, float, f32, f64, i32, i64, u8, u32, u64, str"
            )).into());
        }
    };
    Ok(result)
}

/// Computes the total byte size of a schema without reading any data.
///
/// Used by [`MappedView::read_mixed`] to validate bounds before reading.
pub fn schema_byte_size(schema: &Bound<'_, PyList>) -> PyResult<usize> {
    let mut total = 0usize;
    for item in schema.iter() {
        let tuple = item.cast::<PyTuple>().map_err(|_| {
            ShmError::InvalidArg("schema entries must be (type_tag, count) tuples".into())
        })?;
        let tag: String = tuple.get_item(0)?.extract()?;
        let count = tuple.get_item(1)?.extract::<usize>()?;
        // For "str" we cannot know the size without reading the data,
        // so schema_byte_size is not supported for str columns.
        let elem_size = match tag.as_str() {
            "bool" | "u8"                   => 1,
            "f32"  | "i32" | "u32"          => 4,
            "float"| "f64" | "i64" | "u64"
            | "int"                         => 8,
            "str" => return Err(ShmError::InvalidArg(
                "schema_byte_size cannot compute size for 'str' (variable length);                  use read_range manually".into()
            ).into()),
            other => return Err(ShmError::InvalidArg(
                format!("unknown type tag '{other}'")
            ).into()),
        };
        total += elem_size * count;
    }
    Ok(total)
}