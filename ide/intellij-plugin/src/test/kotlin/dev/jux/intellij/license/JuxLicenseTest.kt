package dev.jux.intellij.license

import com.intellij.ide.util.PropertiesComponent
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxLicense] acceptance state: not accepted by default, sticks after accept,
 * and is keyed to the agreement [JuxLicense.VERSION] so a wording bump
 * re-prompts.
 */
class JuxLicenseTest : BasePlatformTestCase() {

    override fun tearDown() {
        // Don't leak acceptance into other tests sharing the app instance.
        PropertiesComponent.getInstance().unsetValue("dev.jux.license.acceptedVersion")
        super.tearDown()
    }

    fun testNotAcceptedByDefaultThenAccepted() {
        PropertiesComponent.getInstance().unsetValue("dev.jux.license.acceptedVersion")
        assertFalse(JuxLicense.isAccepted())
        JuxLicense.accept()
        assertTrue(JuxLicense.isAccepted())
    }

    fun testStaleVersionIsNotAccepted() {
        // A previously-accepted OTHER version must not count as accepting current.
        PropertiesComponent.getInstance().setValue("dev.jux.license.acceptedVersion", "0")
        assertFalse("a different accepted version must re-prompt", JuxLicense.isAccepted())
    }

    fun testAgreementMentionsAsIsAndNoLiability() {
        val t = JuxLicense.TEXT.lowercase()
        assertTrue(t.contains("as is"))
        assertTrue(t.contains("no liability") || t.contains("liable"))
        assertTrue(JuxLicense.TEXT.contains("juxlang.dev"))
    }
}
