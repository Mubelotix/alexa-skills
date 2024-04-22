use string_tools::get_all_before_strict;

pub async fn get_time_left(stop_id: usize, line_id: usize, sens: usize) -> Result<Option<usize>, String> {
    let url = "https://www.reseau-astuce.fr/fr/horaires-a-larret/28/StopTimeTable/NextDeparture";
    let response = reqwest::Client::new().post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
        .body(format!("destinations=%7B%221%22%3A%22%22%7D&stopId={stop_id}&lineId={line_id}&sens={sens}"))
        .send().await.map_err(|e| format!("Erreur lors de la requête: {e}"))?;
    let response = response.text().await.map_err(|e| format!("Erreur lors de la lecture de la réponse: {e}"))?;
    if response.contains("Pas de prochain") {
        return Ok(None);
    }
    let response = get_all_before_strict(&response, "<abbr title=\"minutes\">")
        .and_then(|s| s.rfind(|c: char| c.is_ascii_digit()).map(|i| &s[i..]))
        .ok_or(String::from("Horaires indisponibles"))?;
    let time = response.parse::<usize>().map_err(|_| String::from("Horaires invalides."))?;

    Ok(Some(time))
}

pub async fn get_stop_id(name: &str, stops: &[(Vec<String>, usize, usize)]) -> Option<usize> {
    let mut min = (None, usize::MAX);
    for stop in stops {
        for stop_name in &stop.0 {
            let distance = levenshtein::levenshtein(stop_name, name);
            if distance < min.1 {
                min = (Some(stop.1), distance);
            }
        }
    }
    min.0
}

pub async fn get_sens(from_stop_id: usize, to_stop_id: usize, stops: &[(Vec<String>, usize, usize)]) -> usize {
    let from_position = stops.iter().position(|(_, stop_id, _)| *stop_id == from_stop_id).unwrap();
    let to_position = stops.iter().position(|(_, stop_id, _)| *stop_id == to_stop_id).unwrap();
    let from_section_id = stops[from_position].2;
    let to_section_id = stops[to_position].2;

    match (from_section_id, to_section_id) {
        (1, 2) | (1, 3) => return 1, // From rouen to georges braque or technopole
        (2, 1) | (3, 1) => return 2, // From georges braque or technopole to rouen
        (2, 3) | (3, 2) => return 1, // From george braque to technopole or vice versa
        _ => () // Same section
    }

    match from_position > to_position {
        true => 2, // If we start south and go north
        false => 1 // If we start north and go south
    }
}
