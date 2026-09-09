mod mnx;
mod render;
mod validate;

use std::fs;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let file_content = fs::read_to_string("example.yaml")?;
    let mnx_doc: mnx::MnxDocument = serde_yaml::from_str(&file_content)?;
    
    println!("Successfully parsed MNX file (Version {})", mnx_doc.mnx.version);
    
    // Run the physical geometry validation
    let errors = validate::validate_mnx(&mnx_doc);
    if !errors.is_empty() {
        println!("\n[!] {} VALIDATION ERRORS FOUND:", errors.len());
        for err in errors {
            println!("  - Measure {}: {}", err.measure_idx, err.message);
        }
        println!("\nCannot render tablature with physical impossibilities. Aborting.");
        return Ok(());
    }

    println!("Validation passed! Geometry matches semantic pitch data.\n");
    
    // Proceed to rendering if valid
    for (i, part) in mnx_doc.parts.iter().enumerate() {
        let instrument_name = part
            .instrument
            .as_ref()
            .map_or("Unknown", |i| i.name.as_str());
            
        println!("Rendering Part {}: {}", i + 1, instrument_name);
        
        let tab = render::render_part_to_tab(part);
        println!("{}", tab);
    }

    Ok(())
}