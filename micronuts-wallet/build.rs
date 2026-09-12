fn main() {
    // Embedded resources keep the software renderer portable (no host font
    // or image lookup at runtime) — see docs/WALLET-UX-DESIGN.md.
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    slint_build::compile_with_config("ui/app.slint", config).expect("failed to compile slint ui");
}
