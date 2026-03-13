import fs from "fs/promises";
import path from "path";

export interface MailEntry {
  id: string;
  timestamp: string;
  from: string;
  subject: string;
  body: string;
  answered: boolean;
  response?: string;
  respondedAt?: string;
}

const MAIL_FILENAME = "MAIL.md";

function parseMailEntries(content: string): MailEntry[] {
  const entries: MailEntry[] = [];
  // Each entry is delimited by a line starting with "## MAIL:"
  const sections = content.split(/^## MAIL:/m).filter((s) => s.trim());

  for (const section of sections) {
    const lines = section.trim().split("\n");
    const headerLine = lines[0].trim();
    // Format: <id> | <timestamp> | from:<from> | subject:<subject>
    const headerMatch = headerLine.match(
      /^(.+?)\s*\|\s*(.+?)\s*\|\s*from:(.+?)\s*\|\s*subject:(.+)$/
    );
    if (!headerMatch) continue;

    const [, id, timestamp, from, subject] = headerMatch;

    const bodyStart = lines.findIndex((l) => l.trim() === "### Body") + 1;
    const responseStart = lines.findIndex((l) => l.trim() === "### Response");

    let body = "";
    let response: string | undefined;
    let respondedAt: string | undefined;
    let answered = false;

    if (bodyStart > 0) {
      const bodyEnd = responseStart > 0 ? responseStart : lines.length;
      body = lines.slice(bodyStart, bodyEnd).join("\n").trim();
    }

    if (responseStart > 0) {
      answered = true;
      const respondedAtLine = lines[responseStart + 1];
      const respondedAtMatch = respondedAtLine?.match(/^_Responded at: (.+)_$/);
      respondedAt = respondedAtMatch?.[1];
      response = lines.slice(responseStart + 2).join("\n").trim();
    }

    entries.push({
      id: id.trim(),
      timestamp: timestamp.trim(),
      from: from.trim(),
      subject: subject.trim(),
      body,
      answered,
      response,
      respondedAt,
    });
  }

  return entries;
}

function formatMailEntry(entry: MailEntry): string {
  let out = `## MAIL: ${entry.id} | ${entry.timestamp} | from:${entry.from} | subject:${entry.subject}\n\n`;
  out += `### Body\n${entry.body}\n\n`;
  if (entry.answered && entry.response) {
    out += `### Response\n_Responded at: ${entry.respondedAt ?? new Date().toISOString()}_\n${entry.response}\n\n`;
  }
  return out;
}

export async function readMail(repoRoot: string): Promise<MailEntry[]> {
  const mailPath = path.join(repoRoot, MAIL_FILENAME);
  try {
    const content = await fs.readFile(mailPath, "utf-8");
    return parseMailEntries(content);
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw err;
  }
}

export async function appendMail(
  repoRoot: string,
  entry: Omit<MailEntry, "id" | "timestamp" | "answered">
): Promise<void> {
  const mailPath = path.join(repoRoot, MAIL_FILENAME);
  const id = `mail-${Date.now()}`;
  const timestamp = new Date().toISOString();

  const fullEntry: MailEntry = {
    ...entry,
    id,
    timestamp,
    answered: false,
  };

  const formatted = formatMailEntry(fullEntry);

  // Ensure file has a header if it doesn't exist
  try {
    await fs.access(mailPath);
  } catch {
    await fs.writeFile(mailPath, "# MAIL.md\n\nThis file contains automated mail from terrarium.\n\n", "utf-8");
  }

  await fs.appendFile(mailPath, formatted, "utf-8");
}

export async function respondToMail(
  repoRoot: string,
  id: string,
  response: string
): Promise<void> {
  const mailPath = path.join(repoRoot, MAIL_FILENAME);
  const content = await fs.readFile(mailPath, "utf-8");
  const entries = parseMailEntries(content);

  const entryIdx = entries.findIndex((e) => e.id === id);
  if (entryIdx < 0) throw new Error(`Mail entry not found: ${id}`);

  entries[entryIdx].answered = true;
  entries[entryIdx].response = response;
  entries[entryIdx].respondedAt = new Date().toISOString();

  const header = content.split(/^## MAIL:/m)[0];
  const newContent =
    header + entries.map(formatMailEntry).join("");

  await fs.writeFile(mailPath, newContent, "utf-8");
}
