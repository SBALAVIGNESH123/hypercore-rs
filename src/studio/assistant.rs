use std::fs::File;
use std::io::Write;
use tracing::info;

pub fn create_assistant(name: &str, model_path: &str) -> anyhow::Result<()> {
    info!("Packaging HyperCore Assistant: {}", name);
    
    let yaml_content = format!(r#"
assistant_name: "{name}"
base_model: "{model}"
lora_adapter: "./my_adapter.lora"
knowledge_base: "./hypercore_knowledge.db"
system_prompt: |
  You are an expert AI assistant named {name}. 
  You have been strictly trained on the user's personal knowledge base.
  Always answer questions based on the retrieved context.
titanmem_enabled: true
"#, name=name, model=model_path);

    let filename = format!("{}.yaml", name.to_lowercase().replace(" ", "_"));
    let mut file = File::create(&filename)?;
    file.write_all(yaml_content.as_bytes())?;
    
    info!("Assistant successfully created at ./{} !", filename);
    info!("Run it anytime with: hypercore run {}", filename);
    
    Ok(())
}
