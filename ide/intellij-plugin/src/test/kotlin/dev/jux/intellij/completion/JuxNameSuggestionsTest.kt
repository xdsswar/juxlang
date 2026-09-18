package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Name suggestions for a declaration being written: the caret after a type
 * offers names made from that type, most fitting first, and nothing else.
 */
class JuxNameSuggestionsTest : BasePlatformTestCase() {

    private fun offered(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testContainerIsNamedForItsElementsFirst() {
        val o = offered("void main() { Vec<String> <caret> }")
        assertEquals(listOf("strings", "vec"), o)
    }

    fun testCamelCaseWordsLongestFirst() {
        myFixture.addFileToProject("http.jux", "public class HttpClient { }")
        val o = offered("void main() { HttpClient <caret> }")
        assertEquals(listOf("httpClient", "client"), o)
    }

    fun testParameterNames() {
        myFixture.addFileToProject("http.jux", "public class HttpClient { }")
        val o = offered("void send(HttpClient <caret>) { }")
        assertEquals(listOf("httpClient", "client"), o)
    }

    fun testFieldNames() {
        myFixture.addFileToProject("http.jux", "public class HttpClient { }")
        val o = offered("public class Api {\n    HttpClient <caret>\n}")
        assertTrue("field names from the type: $o", o.take(2) == listOf("httpClient", "client"))
    }

    fun testTakenNamesGetANumber() {
        myFixture.addFileToProject("http.jux", "public class HttpClient { }")
        val o = offered("void main() { HttpClient client = new HttpClient(); HttpClient <caret> }")
        assertEquals(listOf("httpClient", "client1"), o)
    }

    fun testNameGenerationRules() {
        assertEquals(listOf("ints"), JuxNameSuggestions.namesFor("int[]"))
        assertEquals(listOf("i"), JuxNameSuggestions.namesFor("int"))
        assertEquals(listOf("entries", "list"), JuxNameSuggestions.namesFor("List<Entry>"))
        assertEquals(listOf("hashMap", "map"), JuxNameSuggestions.namesFor("HashMap<String, int>"))
        assertEquals(listOf("httpServer", "server"), JuxNameSuggestions.namesFor("HTTPServer"))
        assertEquals(listOf("boxes", "vec"), JuxNameSuggestions.namesFor("Vec<Box>"))
    }

    fun testANormalStatementIsNotANameSlot() {
        val o = offered("void main() { int total = 1; int totem = 2; tot<caret> }")
        assertTrue("an expression start completes names in scope: $o", o.containsAll(listOf("total", "totem")))
    }
}
