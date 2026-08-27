use crate::{Error, Result};
use std::path::PathBuf;
use std::process::Stdio;

/// Java detectado en el sistema, listo para usarse en `launch`/instalador de Forge.
#[derive(Debug, Clone)]
pub struct JavaInstallation {
    pub executable: PathBuf,
    pub major_version: u32,
}

fn parse_major_version(version_output: &str) -> Option<u32> {
    // "java version \"17.0.9\"" / "openjdk version \"21.0.1\" 2023-10-17" / etc.
    let quoted = version_output.split('"').nth(1)?;
    let first_component = quoted.split('.').next()?;
    let major: u32 = first_component.parse().ok()?;
    // Versiones viejas se anuncian como "1.8.0_x" -> el major real es el segundo número.
    if major == 1 {
        quoted.split('.').nth(1)?.parse().ok()
    } else {
        Some(major)
    }
}

async fn probe(executable: &str) -> Option<JavaInstallation> {
    let output = tokio::process::Command::new(executable)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let major_version = parse_major_version(&stderr)?;

    Some(JavaInstallation {
        executable: PathBuf::from(executable),
        major_version,
    })
}

/// Busca un Java utilizable en JAVA_HOME y en el PATH.
pub async fn find_system_java() -> Option<JavaInstallation> {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let exe = PathBuf::from(&home).join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        if let Some(found) = probe(&exe.to_string_lossy()).await {
            return Some(found);
        }
    }

    probe("java").await
}

/// Confirma que hay un Java >= `required_major` disponible, o devuelve un
/// error explicando cómo resolverlo. Instalar/empaquetar un JRE propio queda
/// como mejora futura (ver README); por ahora dependemos del Java del sistema.
pub async fn ensure_java(required_major: u32) -> Result<JavaInstallation> {
    match find_system_java().await {
        Some(java) if java.major_version >= required_major => Ok(java),
        _ => Err(Error::JavaNotFound { required: required_major }),
    }
}
