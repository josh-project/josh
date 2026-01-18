use crate::service::NamespacedRefs;
use josh_core::{JoshResult, josh_error};
use std::str::FromStr;

#[derive(Debug, Clone, Copy)]
pub enum CapabilitiesDirection {
    UploadPack,
    ReceivePack,
}

impl CapabilitiesDirection {
    pub fn service_name(&self) -> &str {
        match self {
            CapabilitiesDirection::UploadPack => "git-upload-pack",
            CapabilitiesDirection::ReceivePack => "git-receive-pack",
        }
    }

    pub fn content_type(&self) -> &'static str {
        match self {
            CapabilitiesDirection::UploadPack => "application/x-git-upload-pack-advertisement",
            CapabilitiesDirection::ReceivePack => "application/x-git-receive-pack-advertisement",
        }
    }
}

impl FromStr for CapabilitiesDirection {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "git-upload-pack" => Ok(CapabilitiesDirection::UploadPack),
            "git-receive-pack" => Ok(CapabilitiesDirection::ReceivePack),
            _ => Err(()),
        }
    }
}

pub fn git_list_capabilities(
    repo_path: &std::path::Path,
    direction: CapabilitiesDirection,
) -> JoshResult<Vec<String>> {
    use gix_packetline::PacketLineRef;
    use gix_packetline::blocking_io::StreamingPeekableIter;
    use gix_transport::client::capabilities::Capabilities;
    use std::process::Command;

    let service_name = direction.service_name();

    // Invoke git http-backend for info/refs with temporary config
    let output = Command::new("git")
        .arg("-c")
        .arg("http.receivepack=true")
        .arg("http-backend")
        .env("GIT_PROJECT_ROOT", repo_path)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", "/info/refs")
        .env("QUERY_STRING", format!("service={}", service_name))
        .env("REQUEST_METHOD", "GET")
        .output()?;

    if !output.status.success() {
        return Err(josh_error(&format!(
            "git http-backend failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    // Skip HTTP headers until we find the empty line
    let mut cursor = std::io::Cursor::new(&output.stdout);
    let mut header_line = String::new();
    loop {
        header_line.clear();
        std::io::BufRead::read_line(&mut cursor, &mut header_line)?;
        if header_line == "\r\n" || header_line == "\n" {
            break;
        }
    }

    // Now parse packetlines from the remaining buffer
    let mut peekable = StreamingPeekableIter::new(cursor, &[PacketLineRef::ResponseEnd], false);

    // Skip service announcement line
    if let Some(Ok(Ok(_))) = peekable.read_line() {
        // Service announcement
    }

    // Skip first flush
    if let Some(Ok(Ok(PacketLineRef::Flush))) = peekable.read_line() {
        // Flush after service announcement
    }

    // Read first ref line which contains capabilities
    if let Some(Ok(Ok(PacketLineRef::Data(data)))) = peekable.read_line() {
        // Use gix Capabilities parser for V1 protocol
        let (caps, _delimiter_pos) = Capabilities::from_bytes(data)
            .map_err(|e| josh_error(&format!("Failed to parse capabilities: {}", e)))?;

        let capabilities: Vec<String> = caps
            .iter()
            .map(|cap| {
                let name = cap.name().to_string();
                if let Some(value) = cap.value() {
                    let value = value.to_string();
                    let value = value.trim();
                    format!("{}={}", name, value)
                } else {
                    name
                }
            })
            .collect();

        return Ok(capabilities);
    }

    Err(josh_error(
        "Failed to parse capabilities from git http-backend output",
    ))
}

pub fn encode_info_refs(
    namespaced_refs: NamespacedRefs,
    direction: CapabilitiesDirection,
    capabilities: &[String],
) -> JoshResult<Vec<u8>> {
    use gix_packetline::blocking_io::encode;

    let mut output = Vec::new();

    // 1. Service announcement
    let service_line = format!("# service={}", direction.service_name());
    encode::text_to_write(service_line.as_bytes(), &mut output)?;

    // 2. Flush packet
    encode::flush_to_write(&mut output)?;

    let (refs, head_symref) = namespaced_refs.into_inner();

    if !refs.is_empty() {
        let mut caps = capabilities.to_vec();
        let mut sorted_refs = refs;

        // Find the ref HEAD is pointing to
        let head_target_oid = sorted_refs.iter().find_map(|(name, oid)| {
            if name == &head_symref.1 {
                Some(*oid)
            } else {
                None
            }
        });

        // List symref target in caps if HEAD target was present in the list of refs
        if head_target_oid.is_some() {
            caps.push(format!("symref=HEAD:{}", head_symref.1));
        }

        if let Some(oid) = head_target_oid {
            sorted_refs.push(("HEAD".to_string(), oid))
        }

        // Sort refs: HEAD first, then others alphabetically
        sorted_refs.sort_by(|a, b| match (&a.0 == "HEAD", &b.0 == "HEAD") {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        });

        let (first_ref_name, first_ref_target) = &sorted_refs[0];

        // 3. First ref with capabilities (null-separated)
        let first_ref_line = format!(
            "{} {}\0{}\n",
            first_ref_target,
            first_ref_name,
            caps.join(" ")
        );
        encode::data_to_write(first_ref_line.as_bytes(), &mut output)?;

        // 4. Subsequent refs
        for (refname, oid) in &sorted_refs[1..] {
            let ref_line = format!("{} {}\n", oid, refname);
            encode::data_to_write(ref_line.as_bytes(), &mut output)?;
        }
    } else {
        // No valid refs to advertise - send capabilities^{} with NULL OID
        let caps_line = format!(
            "{} capabilities^{{}}\0{}\n",
            gix::ObjectId::null(gix::hash::Kind::Sha1),
            capabilities.join(" ")
        );

        encode::data_to_write(caps_line.as_bytes(), &mut output)?;
    }

    // 5. Final flush packet
    encode::flush_to_write(&mut output)?;

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_cap_discovery() -> anyhow::Result<()> {
        let dir = tempfile::TempDir::new()?;
        let _repo = git2::Repository::init_bare(&dir)?;

        let receive_pack_caps =
            git_list_capabilities(dir.as_ref(), CapabilitiesDirection::ReceivePack).unwrap();
        let upload_pack_caps =
            git_list_capabilities(dir.as_ref(), CapabilitiesDirection::UploadPack).unwrap();

        dbg!(&receive_pack_caps);
        dbg!(&upload_pack_caps);

        assert!(!receive_pack_caps.is_empty());
        assert!(!upload_pack_caps.is_empty());

        Ok(())
    }
}
