pub const OS_API_URL: &str =
    "https://api.github.com/repos/Vertex-Linux/vertex-linux/commits/main";
pub const OS_UPDATE_URL: &str =
    "https://raw.githubusercontent.com/Vertex-Linux/vertex-linux/main/update.sh";

pub const DRIVERS_API: &str =
    "https://api.github.com/repos/Vertex-Linux/vertex-graphics-installer/releases/latest";

// All releases — scanned for a tag starting with "vpkg-"
pub const VPKG_RELEASES_API: &str =
    "https://api.github.com/repos/Vertex-Linux/vertex-tools/releases";

// Scanned for the first release whose tag ends with "-vu"
pub const SELF_RELEASES_API: &str =
    "https://api.github.com/repos/Vertex-Linux/vertex-tools/releases";

// Scanned for the first release whose tag ends with "-vt"
pub const VT_RELEASES_API: &str =
    "https://api.github.com/repos/Vertex-Linux/vertex-tools/releases";

pub const VERSION_FILE: &str = "/etc/vertex-release";
pub const USER_AGENT: &str = "VertexUpdater/2.0";
