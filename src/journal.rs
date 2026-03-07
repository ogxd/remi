use std::io::Write;

/// Writes one commit entry in the standard remi format.
pub fn write_entry(
    file: &mut impl Write,
    short_hash: &str,
    title: &str,
    description: Option<&str>,
    repo: &str,
    time_str: &str,
) -> std::io::Result<()> {
    writeln!(file, "- [{time_str}] Commit {short_hash} on repository \"{repo}\"")?;
    writeln!(file, "  - Message: {title}")?;
    if let Some(desc) = description {
        if !desc.is_empty() {
            let mut lines = desc.lines();
            if let Some(first) = lines.next() {
                writeln!(file, "  - Description: {first}")?;
                for line in lines {
                    if line.trim().is_empty() {
                        writeln!(file)?;
                    } else {
                        writeln!(file, "    {line}")?;
                    }
                }
            }
        }
    }
    Ok(())
}
