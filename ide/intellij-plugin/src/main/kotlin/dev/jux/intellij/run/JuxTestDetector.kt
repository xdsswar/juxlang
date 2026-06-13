package dev.jux.intellij.run

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * Detection of §TS testing-framework functions — the test-side counterpart to
 * [JuxMainDetector].
 *
 * A test is a **free function** (top-level, not a class method) annotated
 * `@Test`; the per-file lifecycle hooks are `@BeforeAll` / `@BeforeEach` /
 * `@AfterEach` / `@AfterAll` (§TS.1). All built-in annotations are
 * case-insensitive (`@test` ≡ `@Test` ≡ `@TEST`), so every comparison here
 * lower-cases first. The compiler is the authoritative check for the full
 * shape rules (`void`/`async void`, no parameters) — see
 * [dev.jux.intellij.inspections.JuxTestAnnotationPlacementInspection] for the
 * IDE-side enforcement.
 */
object JuxTestDetector {
    /**
     * The five §TS.1 annotations: lowercase lookup key → canonical spelling.
     * Single source for detection, completion, and the placement inspection.
     */
    val TEST_HOOKS: Map<String, String> = linkedMapOf(
        "test" to "Test",
        "beforeeach" to "BeforeEach",
        "aftereach" to "AfterEach",
        "beforeall" to "BeforeAll",
        "afterall" to "AfterAll",
    )

    // Cheap pre-PSI gate, same convention as JuxMainDetector.hasMain(text):
    // a line starting with one of the five annotations (any casing).
    private val TEST_ANN_RE = Regex(
        """(?mi)^\s*@(?:test|beforeeach|aftereach|beforeall|afterall)\b""",
    )

    /** True if `text` appears to contain §TS test/hook annotations (regex gate). */
    fun hasTestsText(text: String): Boolean = TEST_ANN_RE.containsMatchIn(text)

    /**
     * The bare annotation names on a declaration (no `@`, no arguments), in
     * source order. ANNOTATION nodes are direct leading children of the
     * declaration — same walk as the missing-`@override` inspection.
     */
    fun annotationNames(decl: PsiElement): List<String> {
        val out = ArrayList<String>()
        var c: PsiElement? = decl.firstChild
        while (c != null) {
            if (c.elementType === JuxElementTypes.ANNOTATION) {
                out.add(c.text.removePrefix("@").substringBefore('(').trim())
            }
            c = c.nextSibling
        }
        return out
    }

    /** True when the method carries the given builtin annotation (case-insensitive). */
    fun hasAnnotation(decl: PsiElement, lowerName: String): Boolean =
        annotationNames(decl).any { it.lowercase() == lowerName }

    /** The method's §TS annotations (canonical spelling), or empty when it has none. */
    fun testAnnotations(decl: PsiElement): List<String> =
        annotationNames(decl).mapNotNull { TEST_HOOKS[it.lowercase()] }

    /** True for a free (top-level) function — tests/hooks are never methods (§TS.1). */
    fun isFreeFunction(method: JuxMethodDeclaration): Boolean = method.parent is JuxFile

    /** True for a free function annotated `@Test` (any casing). */
    fun isTestFunction(method: JuxMethodDeclaration): Boolean =
        isFreeFunction(method) && hasAnnotation(method, "test")

    /** True for a free function annotated with any of the five §TS annotations. */
    fun isTestOrHookFunction(method: JuxMethodDeclaration): Boolean =
        isFreeFunction(method) && testAnnotations(method).isNotEmpty()

    /** The file's `package a.b.c;` name, or `""` when the file has none. */
    fun packageName(file: PsiFile): String {
        val pkg = file.children.firstOrNull { it.elementType === JuxElementTypes.PACKAGE_STATEMENT }
            ?: return ""
        return PACKAGE_RE.find(pkg.text)?.groupValues?.get(1)?.trim().orEmpty()
    }

    /**
     * The test's display name as the runner prints it (§TS.2): its
     * package-qualified function name — `pkg.fn`, or bare `fn` without a package.
     */
    fun qualifiedName(method: JuxMethodDeclaration): String {
        val name = method.name ?: return ""
        val pkg = method.containingFile?.let { packageName(it) }.orEmpty()
        return if (pkg.isEmpty()) name else "$pkg.$name"
    }

    /** True when the file declares at least one top-level `@Test` function. */
    fun hasTests(file: PsiFile): Boolean =
        file.children.any { it is JuxMethodDeclaration && isTestFunction(it) }

    private val PACKAGE_RE = Regex("""package\s+([A-Za-z_][\w.]*)\s*;""")
}
