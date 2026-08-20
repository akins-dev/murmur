/*!
 * SOURCE OF TRUTH KEYWORDS: MODEL_CATALOG, catalog, descriptor_for,
 *   DEFAULT_MODEL_ID, FALLBACK_MODEL_ID, HF_BASE_URL, CatalogEntry
 * WHAT:  The static table of every model Murmur offers, with its URL, size and
 *        SHA-256.
 * WHY:   A static table rather than a fetched manifest, because listing models
 *        has to work on a plane. The whole model manager — names, sizes, RAM
 *        warnings, which one is default — is local data plus a hash check of
 *        what is on disk, so the only thing that ever needs the network is the
 *        download itself. The hashes are the upstream Git-LFS object ids read
 *        from Hugging Face's `X-Linked-Etag`; they are what makes a truncated
 *        574MB file a caught error instead of a crash inside inference.
 * WHERE: Read by adapters/http_models/store.rs; surfaced through the ModelStore
 *        port. Assets listed in docs/03-IMPLEMENTATION-NOTES.md §2.7.
 */

use crate::types::{ModelDescriptor, ModelId};

/// Every file below is resolved against this. whisper.cpp's own repository.
pub const HF_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

/// The model the app installs unless the user chooses otherwise.
pub const DEFAULT_MODEL_ID: &str = "large-v3-turbo-q5_0";
/// Offered when the default will not fit comfortably in RAM.
pub const FALLBACK_MODEL_ID: &str = "small-q5_1";

/**
 * SOURCE OF TRUTH KEYWORDS: CatalogEntry
 * WHAT:  One row of the table, in the shape a `const` can hold.
 * WHY:   ModelDescriptor owns `String`s so it can cross IPC, which no `const`
 *        can build. This is the same data with `&'static str`, converted once
 *        on read — rather than a lazily-initialised global, which would be a
 *        second source of truth with a lifetime.
 * WHERE: MODEL_CATALOG; converted by CatalogEntry::descriptor.
 */
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub file_name: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub approx_ram_mb: u64,
    pub is_default: bool,
}

impl CatalogEntry {
    pub fn descriptor(&self) -> ModelDescriptor {
        ModelDescriptor {
            id: ModelId(self.id.to_string()),
            display_name: self.display_name.to_string(),
            description: self.description.to_string(),
            url: format!("{HF_BASE_URL}{}", self.file_name),
            sha256: self.sha256.to_string(),
            size_bytes: self.size_bytes,
            approx_ram_mb: self.approx_ram_mb,
            is_default: self.is_default,
        }
    }
}

/// The table. Ordered as the model manager should list it: default first.
pub const MODEL_CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: DEFAULT_MODEL_ID,
        display_name: "Large v3 Turbo",
        description: "The default. Full accuracy across all 99 languages, quantised so it stays fast on Apple silicon.",
        file_name: "ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 574_041_195,
        approx_ram_mb: 1_100,
        is_default: true,
    },
    CatalogEntry {
        id: FALLBACK_MODEL_ID,
        display_name: "Small",
        description: "A third of the size and memory. Noticeably less accurate on accents and on languages other than English.",
        file_name: "ggml-small-q5_1.bin",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
        size_bytes: 190_085_487,
        approx_ram_mb: 450,
        is_default: false,
    },
];

/// The catalog entry with this id, if the app offers it.
pub fn descriptor_for(id: &ModelId) -> Option<&'static CatalogEntry> {
    MODEL_CATALOG.iter().find(|entry| entry.id == id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_exactly_one_default() {
        let defaults = MODEL_CATALOG.iter().filter(|e| e.is_default).count();
        assert_eq!(defaults, 1);
        assert!(descriptor_for(&ModelId(DEFAULT_MODEL_ID.into())).is_some_and(|e| e.is_default));
    }

    #[test]
    fn the_fallback_is_offered_and_is_smaller() {
        let default = descriptor_for(&ModelId(DEFAULT_MODEL_ID.into())).expect("default listed");
        let fallback = descriptor_for(&ModelId(FALLBACK_MODEL_ID.into())).expect("fallback listed");
        assert!(fallback.size_bytes < default.size_bytes);
        assert!(fallback.approx_ram_mb < default.approx_ram_mb);
    }

    #[test]
    fn every_entry_carries_a_full_sha256_and_a_reachable_shaped_url() {
        for entry in MODEL_CATALOG {
            assert_eq!(entry.sha256.len(), 64, "{} sha256 is not 32 bytes", entry.id);
            assert!(
                entry.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} sha256 is not hex",
                entry.id
            );
            assert!(entry.size_bytes > 0);
            let descriptor = entry.descriptor();
            assert!(descriptor.url.starts_with(HF_BASE_URL));
            assert!(descriptor.url.ends_with(".bin"));
        }
    }

    #[test]
    fn ids_are_unique_because_they_name_files_on_disk() {
        let mut ids: Vec<&str> = MODEL_CATALOG.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count);
    }

    #[test]
    fn an_unknown_id_is_not_invented() {
        assert!(descriptor_for(&ModelId("ggml-imaginary".into())).is_none());
    }
}
