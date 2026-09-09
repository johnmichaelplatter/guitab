mod mnx;
mod render;

use std::fs;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let file_content = fs::read_to_string("example.yaml")?;
    
    // Parse the YAML explicitly into our MNX data model
    let mnx_doc: mnx::MnxDocument = serde_yaml::from_str(&file_content)?;
    
    println!("Successfully parsed MNX file (Version {})", mnx_doc.mnx.version);
    
    for (i, part) in mnx_doc.parts.iter().enumerate() {
        let instrument_name = part
            .instrument
            .as_ref()
            .map_or("Unknown", |i| i.name.as_str());
            
        println!("\nRendering Part {}: {}", i + 1, instrument_name);
        
        let tab = render::render_part_to_tab(part);
        println!("{}", tab);
    }

    Ok(())
}