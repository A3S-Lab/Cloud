pub fn dsse_pae_bounded(
    payload_type: &str,
    payload: &[u8],
    maximum_payload_bytes: usize,
) -> Result<Vec<u8>, String> {
    if maximum_payload_bytes == 0
        || payload_type.is_empty()
        || payload_type.len() > 255
        || !payload_type
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b' ')
        || payload.len() > maximum_payload_bytes
    {
        return Err("DSSE payload type or body exceeds its protocol bounds".into());
    }
    let prefix = format!(
        "DSSEv1 {} {} {} ",
        payload_type.len(),
        payload_type,
        payload.len()
    );
    let mut pae = Vec::with_capacity(prefix.len().saturating_add(payload.len()));
    pae.extend_from_slice(prefix.as_bytes());
    pae.extend_from_slice(payload);
    Ok(pae)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pae_is_length_delimited_and_bounded() {
        assert_eq!(
            dsse_pae_bounded("application/example+json", b"{}", 2).expect("PAE"),
            b"DSSEv1 24 application/example+json 2 {}".to_vec()
        );
        assert!(dsse_pae_bounded("application/example+json", b"{}", 1).is_err());
        assert!(dsse_pae_bounded("application/example json", b"{}", 2).is_err());
    }
}
