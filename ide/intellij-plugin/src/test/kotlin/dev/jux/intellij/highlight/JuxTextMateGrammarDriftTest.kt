package dev.jux.intellij.highlight

import com.google.gson.JsonElement
import com.google.gson.JsonParser
import junit.framework.TestCase
import java.io.File

/**
 * The shared TextMate grammar (`editors/jux.tmLanguage.json`) is written by
 * hand, and the lexer's keyword list is not, so the two drift the moment the
 * language gains a keyword: `ref`, `typeof` and `weak` went unhighlighted, and
 * `match`, a Rust word, was painted as a Jux one (GAPS 22). This pins both
 * directions against `grammar/jux-tokens.json`, the list the compiler exports
 * and this plugin's own token types are generated from.
 *
 * It also pins the grammar's copies: the TextMate bundle's
 * `Syntaxes/jux.tmLanguage.json` is a byte-for-byte copy of the shared file,
 * since a bundle host loads it from there (JUX-EDITOR-TOOLING-ADDENDUM §E.5).
 * The VS Code extension copies the file at package time, so it has no
 * committed copy to check.
 */
class JuxTextMateGrammarDriftTest : TestCase() {

    private val repo = File("../..")
    private val grammarFile = File(repo, "editors/jux.tmLanguage.json")

    /** Every keyword the lexer reserves. */
    private fun lexerKeywords(): Set<String> {
        val tokens = JsonParser.parseString(File("grammar/jux-tokens.json").readText()).asJsonObject
        return tokens.getAsJsonArray("keywords").map { it.asJsonObject.get("spelling").asString }.toSet()
    }

    /** Each `\b(a|b|c)\b` word list in the grammar, with the scope it is painted in. */
    private fun grammarWords(): List<Pair<String, Set<String>>> {
        val out = ArrayList<Pair<String, Set<String>>>()
        fun walk(e: JsonElement) {
            when {
                e.isJsonArray -> e.asJsonArray.forEach(::walk)
                e.isJsonObject -> {
                    val o = e.asJsonObject
                    val match = o.get("match")?.takeIf { it.isJsonPrimitive }?.asString
                    val words = match?.let { WORD_LIST.matchEntire(it) }?.groupValues?.get(1)
                    if (words != null) out.add((o.get("name")?.asString ?: "") to words.split('|').toSet())
                    o.entrySet().forEach { walk(it.value) }
                }
            }
        }
        walk(JsonParser.parseString(grammarFile.readText()))
        return out
    }

    fun testEveryLexerKeywordIsHighlighted() {
        val highlighted = grammarWords().flatMap { it.second }.toSet()
        val missing = lexerKeywords() - highlighted
        assertTrue("keywords the TextMate grammar does not highlight: $missing", missing.isEmpty())
    }

    fun testNoKeywordScopeHighlightsANonKeyword() {
        val keywords = lexerKeywords()
        val stray = grammarWords()
            .filter { (scope, _) -> KEYWORD_SCOPES.any { scope.startsWith(it) } }
            .flatMap { it.second }
            .filter { it !in keywords }
        assertTrue("words painted as Jux keywords that the lexer does not reserve: $stray", stray.isEmpty())
    }

    fun testTheBundleCarriesTheSharedGrammar() {
        val copy = File(repo, "editors/jux.tmbundle/Syntaxes/jux.tmLanguage.json")
        assertTrue("the TextMate bundle has no Syntaxes/jux.tmLanguage.json", copy.isFile)
        assertTrue(
            "editors/jux.tmbundle/Syntaxes/jux.tmLanguage.json differs from editors/jux.tmLanguage.json; copy it over",
            copy.readBytes().contentEquals(grammarFile.readBytes()),
        )
    }

    private companion object {
        val WORD_LIST = Regex("""\\b\(([A-Za-z0-9_|]+)\)\\b""")
        val KEYWORD_SCOPES = listOf("keyword.", "storage.modifier", "variable.language")
    }
}
