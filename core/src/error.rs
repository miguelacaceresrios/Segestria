use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("error de red: {0}")]
    Network(#[from] reqwest::Error),

    #[error("error de disco: {0}")]
    Io(#[from] std::io::Error),

    #[error("error de formato (json): {0}")]
    Json(#[from] serde_json::Error),

    #[error("error al descomprimir {path}: {source}")]
    Zip {
        path: PathBuf,
        #[source]
        source: zip::result::ZipError,
    },

    #[error("checksum inválido para {path}: esperado {expected}, obtenido {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("no se encontró la versión de Minecraft \"{0}\"")]
    VersionNotFound(String),

    #[error("no se encontró Java {required}+ instalado. Instalá Java {required} o superior (por ejemplo desde https://adoptium.net) y volvé a intentar.")]
    JavaNotFound { required: u32 },

    #[error("no se pudo resolver una build de Forge para Minecraft {0}")]
    ForgeVersionNotFound(String),

    #[error("el instalador de Forge falló (código de salida {code:?}): {details}")]
    ForgeInstallFailed { code: Option<i32>, details: String },

    #[error("no se encontró el version.json generado por Forge en {0}")]
    ForgeVersionJsonMissing(PathBuf),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
