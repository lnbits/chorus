fn main() -> anyhow::Result<()> {
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/echo.wasm".into());
    let bytes = wat::parse_str(include_str!("../../../examples/echo.wat"))?;
    chorus_fedvm::FedVm::new()?.validate(&bytes, &chorus_protocol::hash(&bytes))?;
    std::fs::write(&output, &bytes)?;
    println!("{}  {}", chorus_protocol::hash(&bytes), output);
    Ok(())
}
