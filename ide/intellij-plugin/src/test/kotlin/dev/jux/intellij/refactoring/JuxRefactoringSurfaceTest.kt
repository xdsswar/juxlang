package dev.jux.intellij.refactoring

import com.intellij.lang.parameterInfo.LanguageParameterInfo
import com.intellij.lang.refactoring.InlineActionHandler
import com.intellij.lang.LanguageRefactoringSupport
import com.intellij.lang.LanguageSurrounders
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.safeDelete.SafeDeleteProcessorDelegate
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * The registration contract for this release's new surfaces.
 *
 * Every feature here is reached through an extension point, which means the
 * implementation can be perfect and the feature still not exist: a typo'd
 * `plugin.xml` entry, or a class renamed without its registration, compiles and
 * ships and simply does nothing. This test is the same guard `juxc-lsp` puts on
 * its advertised capabilities, for the same reason.
 */
class JuxRefactoringSurfaceTest : BasePlatformTestCase() {

    fun testTheRefactoringGateIsRegistered() {
        // Without this provider the platform greys out Extract Variable,
        // Introduce Constant and Safe Delete no matter what else is registered.
        val provider = LanguageRefactoringSupport.getInstance().forLanguage(JuxLanguage)
        assertNotNull("lang.refactoringSupport is not registered for Jux", provider)
        assertTrue(provider is JuxRefactoringSupportProvider)
        assertNotNull(provider.introduceVariableHandler)
        assertNotNull(provider.introduceConstantHandler)
    }

    fun testSafeDeleteHandlesJuxDeclarations() {
        myFixture.configureByText("a.jux", "class C { void m() { var x = 1; } }")
        val delegate = SafeDeleteProcessorDelegate.EP_NAME.extensionList
            .filterIsInstance<JuxSafeDeleteProcessor>()
            .firstOrNull()
        assertNotNull("refactoring.safeDeleteProcessor is not registered for Jux", delegate)

        val type = myFixture.file.children.filterIsInstance<JuxTypeDeclaration>().first()
        val local = PsiTreeUtil.findChildOfType(myFixture.file, JuxLocalVariable::class.java)!!
        assertTrue("a type should be safe-deletable", delegate!!.handlesElement(type))
        assertTrue("a local should be safe-deletable", delegate.handlesElement(local))
    }

    fun testInlineIsRegisteredForJux() {
        val handler = InlineActionHandler.EP_NAME.extensionList
            .filterIsInstance<JuxInlineVariableHandler>()
            .firstOrNull()
        assertNotNull("inlineActionHandler is not registered for Jux", handler)
        assertTrue(handler!!.isEnabledForLanguage(JuxLanguage))
    }

    fun testParameterInfoIsRegisteredForJux() {
        assertNotNull(
            "lang.parameterInfo is not registered for Jux",
            LanguageParameterInfo.INSTANCE.allForLanguage(JuxLanguage).firstOrNull(),
        )
    }

    fun testSurroundDescriptorsAreRegisteredForJux() {
        val descriptors = LanguageSurrounders.INSTANCE.allForLanguage(JuxLanguage)
        assertTrue(
            "expected both the statement and expression surround descriptors, got $descriptors",
            descriptors.size >= 2,
        )
    }
}
