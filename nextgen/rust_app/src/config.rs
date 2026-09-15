#[derive(Clone, Debug, PartialEq)]
pub struct CampaignConfig {
    pub name: String,
    pub targets: Vec<CampaignTargetConfig>,
    pub bias: f64,
    pub dark_current: f64,
    pub flat_field: f64,
    pub threshold: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CampaignTargetConfig {
    pub name: String,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub priority: u8,
}

pub fn parse_campaign_config(text: &str) -> Result<CampaignConfig, String> {
    let mut name = String::new();
    let mut targets = Vec::new();
    let mut bias = 0.0_f64;
    let mut dark_current = 0.0_f64;
    let mut flat_field = 1.0_f64;
    let mut threshold = 25.0_f64;
    let mut section = "";

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }

        if section == "target" && !line.contains('=') {
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();
            if parts.len() < 4 {
                return Err(format!("invalid target row: '{raw_line}'"));
            }

            let target_name = parts[0];
            let ra_deg = parts[1].parse::<f64>().map_err(|_| format!("invalid ra_deg in '{raw_line}'"))?;
            let dec_deg = parts[2].parse::<f64>().map_err(|_| format!("invalid dec_deg in '{raw_line}'"))?;
            let priority = parts[3].parse::<u8>().map_err(|_| format!("invalid priority in '{raw_line}'"))?;

            if !target_name.is_empty() {
                targets.push(CampaignTargetConfig {
                    name: target_name.to_string(),
                    ra_deg,
                    dec_deg,
                    priority,
                });
            }
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("invalid config line: '{raw_line}'"));
        };

        let key = key.trim();
        let value = value.trim();

        match section {
            "campaign" => match key {
                "name" => name = value.to_string(),
                "bias" => bias = value.parse::<f64>().map_err(|_| format!("invalid bias: '{value}'"))?,
                "dark_current" => dark_current = value.parse::<f64>().map_err(|_| format!("invalid dark_current: '{value}'"))?,
                "flat_field" => flat_field = value.parse::<f64>().map_err(|_| format!("invalid flat_field: '{value}'"))?,
                "threshold" => threshold = value.parse::<f64>().map_err(|_| format!("invalid threshold: '{value}'"))?,
                _ => {}
            },
            _ => {
                if key == "name" && name.is_empty() {
                    name = value.to_string();
                }
            }
        }
    }

    if name.is_empty() {
        return Err("campaign name is required".to_string());
    }

    Ok(CampaignConfig {
        name,
        targets,
        bias,
        dark_current,
        flat_field,
        threshold,
    })
}

pub fn config_to_targets(config: &CampaignConfig) -> Vec<crate::CampaignTarget> {
    config
        .targets
        .iter()
        .map(|target| crate::CampaignTarget {
            name: Box::leak(target.name.clone().into_boxed_str()),
            ra_deg: target.ra_deg,
            dec_deg: target.dec_deg,
            priority: target.priority,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_campaign_config_reads_campaign_and_targets() {
        let text = r#"
[campaign]
name=NGC_Example
bias=5.0
dark_current=1.0
flat_field=2.0
threshold=25.0

[target]
M31,10.6847,41.2687,5
M45,56.75,24.1167,3
"#;

        let config = parse_campaign_config(text).unwrap();
        assert_eq!(config.name, "NGC_Example");
        assert_eq!(config.targets.len(), 2);
        assert_eq!(config.targets[0].name, "M31");
        assert_eq!(config.threshold, 25.0);
    }
}
