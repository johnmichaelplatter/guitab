#[derive(Debug, Clone, Copy)]
pub struct EdoTuning {
    pub edo: u32,
    pub generator: i32,     
    pub sharp_steps: i32,   
    pub step_semitones: f64,
}

impl EdoTuning {
    pub fn new(edo: u32) -> Self {
        let e = edo as f64;
        let generator = (e * 1.5_f64.log2()).round() as i32;
        let sharp_steps = 7 * generator - 4 * (edo as i32);
        
        Self {
            edo,
            generator,
            sharp_steps,
            step_semitones: 12.0 / e,
        }
    }

    pub fn nominal_step(&self, step_char: char) -> i32 {
        let g = self.generator;
        let e = self.edo as i32;
        
        match step_char {
            'C' => 0,
            'D' => (2 * g).rem_euclid(e),
            'E' => (4 * g).rem_euclid(e),
            'F' => (-g).rem_euclid(e),
            'G' => g.rem_euclid(e),
            'A' => (3 * g).rem_euclid(e),
            'B' => (5 * g).rem_euclid(e),
            _ => 0,
        }
    }
}

pub fn generate_edo_scale(edo: u32) -> Vec<(i32, String, f64, f64)> {
    let tuning = EdoTuning::new(edo);
    let e = edo as i32;
    let mut scale = Vec::new();
    
    let nominals = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
    
    for step in 0..e {
        let mut best_name = String::new();
        let mut best_score = i32::MAX;
        let mut best_alter = 0.0;
        
        for nom_char in &nominals {
            let base_step = tuning.nominal_step(*nom_char);
            
            let mut dist = (step - base_step).rem_euclid(e);
            if dist > e / 2 { dist -= e; }
            
            for sharps in -2..=2 {
                let ups = dist - sharps * tuning.sharp_steps;
                let score = sharps.abs() * 2 + ups.abs() * 3;
                
                if score < best_score {
                    best_score = score;
                    
                    let mut name = String::new();
                    if ups > 0 { name.push_str(&"^".repeat(ups as usize)); }
                    if ups < 0 { name.push_str(&"v".repeat((-ups) as usize)); }
                    
                    name.push(*nom_char);
                    
                    if sharps > 0 { name.push_str(&"#".repeat(sharps as usize)); }
                    if sharps < 0 { name.push_str(&"b".repeat((-sharps) as usize)); }
                    
                    best_name = name;
                    
                    let alter_steps = sharps * tuning.sharp_steps + ups;
                    best_alter = (alter_steps as f64) * tuning.step_semitones;
                }
            }
        }
        
        let midi_note = 60.0 + (step as f64) * tuning.step_semitones;
        scale.push((step, best_name, best_alter, midi_note));
    }
    
    scale
}