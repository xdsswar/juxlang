package dev.jux.intellij.toml

import java.io.File

/**
 * A line-level model of a `jux.toml` manifest: which table each line belongs
 * to, the key/value lines inside it, and the workspace facts the editor
 * checks (JUX-BUILD-SYSTEM-ADDENDUM §B.7). Pure text in, data out, so it
 * works whether the IDE reads `jux.toml` as TOML (with the TOML plugin) or as
 * plain text, and it unit-tests without a fixture.
 *
 * It reads the same shapes the driver accepts: `[workspace]` with `members`,
 * `exclude`, `default-members` (patterns may use `*` and `?`, one path
 * segment at a time); `[workspace.package]` / `[workspace.dependencies]` in
 * the root; and a member's `key.workspace = true` (dotted key) or
 * `key = { workspace = true, ... }` (inline table) inheritance markers.
 */
object JuxTomlModel {

    /** Keys the `[workspace]` table takes (§B.7.1); the two sub-tables are headers. */
    val WORKSPACE_KEYS: List<String> = listOf("members", "exclude", "default-members")

    /**
     * `[package]` keys a member may take from `[workspace.package]`: every
     * package key but the package's own `name`, which is per member.
     */
    val INHERITABLE_PACKAGE_KEYS: List<String> = listOf(
        "version", "edition", "description", "authors", "license",
        "homepage", "repository", "company", "copyright", "icon",
    )

    /** One `key = value` line of the manifest. */
    data class Entry(
        /** The table the line belongs to (`workspace`, `dependencies`, `profile.fast`), `""` at the top. */
        val table: String,
        /** The whole key as written, dotted parts joined (`edition.workspace`), quotes removed. */
        val key: String,
        /** The first key segment (`edition`, or `com.x.json` for `"com.x.json".workspace`). */
        val head: String,
        /** The value text after `=`, trimmed, comment removed. */
        val value: String,
        /** Offset of the first character of the key in the manifest text. */
        val keyStart: Int,
        /** Offset just past the key's head segment. */
        val headEnd: Int,
        /** Offset of the value's first character. */
        val valueStart: Int,
        /** Offset just past the value. */
        val valueEnd: Int,
    ) {
        /** True for `key.workspace = true` or `key = { workspace = true, ... }`. */
        val inheritsFromWorkspace: Boolean
            get() = (key == "$head.workspace" && value == "true") ||
                INLINE_WORKSPACE.containsMatchIn(value)
    }

    private val HEADER = Regex("""^\s*\[\[?\s*([^\]]+?)\s*]]?\s*(?:#.*)?$""")
    private val INLINE_WORKSPACE = Regex("""^\{.*\bworkspace\s*=\s*true\b.*}$""")

    /** The table header name of a header line (`[workspace.package]` -> `workspace.package`), else null. */
    fun headerOf(line: String): String? =
        HEADER.matchEntire(line)?.groupValues?.get(1)?.let(::normalizeKey)

    /** Every `key = value` line with the table it sits in. Multi-line arrays are joined. */
    fun entries(text: String): List<Entry> {
        val out = ArrayList<Entry>()
        var table = ""
        var offset = 0
        val lines = text.split('\n')
        var i = 0
        while (i < lines.size) {
            val line = lines[i]
            val lineStart = offset
            offset += line.length + 1
            i++
            val header = headerOf(line)
            if (header != null) {
                table = header
                continue
            }
            val code = stripComment(line)
            val eq = equalsIndex(code)
            if (eq <= 0) continue
            val rawKey = code.substring(0, eq)
            val keyLead = rawKey.length - rawKey.trimStart().length
            val key = normalizeKey(rawKey.trim())
            if (key.isEmpty()) continue
            var valueText = code.substring(eq + 1)
            val valueLead = valueText.length - valueText.trimStart().length
            var valueEnd = lineStart + code.trimEnd().length
            // An array spanning several lines runs until its brackets balance.
            while (bracketDepth(valueText) > 0 && i < lines.size) {
                val more = stripComment(lines[i])
                valueText += "\n" + more
                valueEnd = offset + more.trimEnd().length
                offset += lines[i].length + 1
                i++
            }
            val head = firstSegment(rawKey.trim())
            val keyStart = lineStart + keyLead
            out.add(
                Entry(
                    table = table,
                    key = key,
                    head = head,
                    value = valueText.trim(),
                    keyStart = keyStart,
                    headEnd = keyStart + rawHeadLength(rawKey.trim()),
                    valueStart = lineStart + eq + 1 + valueLead,
                    valueEnd = valueEnd,
                ),
            )
        }
        return out
    }

    /** The table the character at [offset] belongs to (`""` before any header). */
    fun tableAt(text: String, offset: Int): String {
        var table = ""
        var at = 0
        for (line in text.split('\n')) {
            if (at > offset) break
            headerOf(line)?.let { table = it }
            at += line.length + 1
        }
        return table
    }

    /** Every table header the manifest declares, in order. */
    fun tables(text: String): List<String> = text.split('\n').mapNotNull(::headerOf)

    /** The strings of a TOML array value (`["a", "b"]`); empty for anything else. */
    fun stringArray(value: String): List<String> =
        Regex(""""((?:[^"\\]|\\.)*)"|'([^']*)'""").findAll(value.substringAfter('[', "").substringBeforeLast(']'))
            .map { m -> m.groupValues[1].ifEmpty { m.groupValues[2] } }
            .toList()

    /** The `[workspace]` array under [key] (`members`, `exclude`, `default-members`). */
    fun workspaceArray(text: String, key: String): List<String> =
        entries(text).firstOrNull { it.table == "workspace" && it.key == key }?.let { stringArray(it.value) }.orEmpty()

    /** True when the manifest is a workspace root (it has a `[workspace]` table). */
    fun isWorkspaceRoot(text: String): Boolean = tables(text).any { it == "workspace" }

    /** The keys a root offers in `[workspace.package]`. */
    fun workspacePackageKeys(rootText: String): List<String> =
        entries(rootText).filter { it.table == "workspace.package" }.map { it.head }.distinct()

    /**
     * The dependency names a root offers in `[workspace.dependencies]`: inline
     * entries plus `[workspace.dependencies.NAME]` sub-tables.
     */
    fun workspaceDependencyNames(rootText: String): List<String> {
        val inline = entries(rootText).filter { it.table == "workspace.dependencies" }.map { it.head }
        val tables = tables(rootText).filter { it.startsWith("workspace.dependencies.") }
            .map { it.removePrefix("workspace.dependencies.") }
        return (inline + tables).distinct()
    }

    /**
     * Where [key] is declared in the root's `[workspace.package]` (for a
     * package key) or `[workspace.dependencies]` (for a dependency): the
     * offset of the key, or null. Used by go-to-declaration.
     */
    fun workspaceDeclarationOffset(rootText: String, memberTable: String, key: String): Int? {
        val target = when (memberTable) {
            "package" -> "workspace.package"
            "dependencies" -> "workspace.dependencies"
            else -> return null
        }
        entries(rootText).firstOrNull { it.table == target && it.head == key }?.let { return it.keyStart }
        if (target == "workspace.dependencies") {
            // A `[workspace.dependencies.NAME]` sub-table: point at its header.
            var at = 0
            for (line in rootText.split('\n')) {
                if (headerOf(line) == "workspace.dependencies.$key") return at + line.indexOf('[').coerceAtLeast(0)
                at += line.length + 1
            }
        }
        return null
    }

    // ---- Workspace members on disk (mirrors juxc-driver workspace.rs) --------

    /**
     * A read-only view of the directory tree under a workspace root, so the
     * member checks run the same over `java.io.File` and the IDE's virtual
     * files. Paths are `/`-separated and relative to the root (`""` = root).
     */
    interface DirView {
        /** The names of the sub-directories of [rel], or empty. */
        fun childDirs(rel: String): List<String>

        /** True when [rel] is a directory. */
        fun isDirectory(rel: String): Boolean

        /** True when [rel] holds a `jux.toml`. */
        fun hasManifest(rel: String): Boolean
    }

    /** A [DirView] over a real directory. */
    class FileDirView(private val root: File) : DirView {
        private fun at(rel: String) = if (rel.isEmpty()) root else File(root, rel)
        override fun childDirs(rel: String): List<String> =
            at(rel).listFiles()?.filter { it.isDirectory }?.map { it.name }.orEmpty()
        override fun isDirectory(rel: String): Boolean = at(rel).isDirectory
        override fun hasManifest(rel: String): Boolean = File(at(rel), "jux.toml").isFile
    }

    /**
     * Expand `members` patterns against [root] and drop what `exclude`
     * names, the way the driver does: a plain entry is kept as written, a
     * pattern only yields directories holding a `jux.toml`.
     */
    fun expandMembers(root: File, members: List<String>, exclude: List<String>): List<String> =
        expandMembers(FileDirView(root), members, exclude)

    /** [expandMembers] over any [DirView]. */
    fun expandMembers(root: DirView, members: List<String>, exclude: List<String>): List<String> {
        val out = LinkedHashSet<String>()
        for (entry in members.map(::normalizePath)) {
            if (hasWildcard(entry)) {
                expandPattern(root, entry).filter { root.hasManifest(it) }.forEach { out.add(it) }
            } else {
                out.add(entry)
            }
        }
        val ex = exclude.map(::normalizePath)
        return out.filter { m -> ex.none { pathMatches(it, m) } }
    }

    /** True when a `default-members` / `exclude` [pattern] names member path [member]. */
    fun pathMatches(pattern: String, member: String): Boolean {
        val p = normalizePath(pattern).split('/')
        val m = normalizePath(member).split('/')
        if (p.size != m.size) return false
        return p.zip(m).all { (a, b) -> segmentMatches(a, b) }
    }

    fun hasWildcard(s: String): Boolean = s.contains('*') || s.contains('?')

    fun normalizePath(s: String): String =
        s.replace('\\', '/').trim().removePrefix("./").trimEnd('/')

    /** Every directory under [root] a `/`-separated wildcard [pattern] names. */
    fun expandPattern(root: DirView, pattern: String): List<String> {
        var paths = listOf("")
        for (segment in pattern.split('/')) {
            val next = ArrayList<String>()
            for (base in paths) {
                if (!hasWildcard(segment)) {
                    val child = if (base.isEmpty()) segment else "$base/$segment"
                    if (root.isDirectory(child)) next.add(child)
                    continue
                }
                root.childDirs(base).filter { segmentMatches(segment, it) }
                    .map { if (base.isEmpty()) it else "$base/$it" }
                    .sorted()
                    .let(next::addAll)
            }
            paths = next
        }
        return paths
    }

    /** Glob match of one path segment: `*` any run, `?` one character. */
    private fun segmentMatches(pattern: String, name: String): Boolean {
        val regex = buildString {
            append('^')
            for (c in pattern) {
                when (c) {
                    '*' -> append(".*")
                    '?' -> append('.')
                    else -> append(Regex.escape(c.toString()))
                }
            }
            append('$')
        }
        return Regex(regex).matches(name)
    }

    // ---- Text helpers --------------------------------------------------------

    /** The line without a trailing `# comment` (a `#` inside a string is kept). */
    private fun stripComment(line: String): String {
        var inString: Char? = null
        for ((i, c) in line.withIndex()) {
            when {
                inString != null && c == inString && (i == 0 || line[i - 1] != '\\') -> inString = null
                inString == null && (c == '"' || c == '\'') -> inString = c
                inString == null && c == '#' -> return line.substring(0, i)
            }
        }
        return line
    }

    /** Index of the `=` that separates key from value, outside quotes; -1 if none. */
    private fun equalsIndex(code: String): Int {
        var inString: Char? = null
        for ((i, c) in code.withIndex()) {
            when {
                inString != null && c == inString -> inString = null
                inString == null && (c == '"' || c == '\'') -> inString = c
                inString == null && c == '=' -> return i
            }
        }
        return -1
    }

    private fun bracketDepth(value: String): Int {
        var depth = 0
        var inString: Char? = null
        for (c in value) {
            when {
                inString != null && c == inString -> inString = null
                inString == null && (c == '"' || c == '\'') -> inString = c
                inString == null && (c == '[' || c == '{') -> depth++
                inString == null && (c == ']' || c == '}') -> depth--
            }
        }
        return depth
    }

    /** `"com.x.json".workspace` -> `com.x.json.workspace`; `edition . workspace` -> `edition.workspace`. */
    private fun normalizeKey(key: String): String =
        splitKey(key).joinToString(".")

    private fun firstSegment(key: String): String = splitKey(key).firstOrNull().orEmpty()

    /** Length of the first key segment as written, quotes included. */
    private fun rawHeadLength(key: String): Int {
        if (key.startsWith('"') || key.startsWith('\'')) {
            val close = key.indexOf(key[0], 1)
            return if (close > 0) close + 1 else key.length
        }
        val dot = key.indexOf('.')
        return (if (dot < 0) key.length else dot).let { n -> key.substring(0, n).trimEnd().length }
    }

    /** Dotted key segments, honoring quoted segments that contain dots. */
    private fun splitKey(key: String): List<String> {
        val out = ArrayList<String>()
        val cur = StringBuilder()
        var inString: Char? = null
        for (c in key) {
            when {
                inString != null && c == inString -> inString = null
                inString != null -> cur.append(c)
                c == '"' || c == '\'' -> inString = c
                c == '.' -> {
                    out.add(cur.toString().trim()); cur.clear()
                }
                else -> cur.append(c)
            }
        }
        out.add(cur.toString().trim())
        return out.filter { it.isNotEmpty() }
    }
}
