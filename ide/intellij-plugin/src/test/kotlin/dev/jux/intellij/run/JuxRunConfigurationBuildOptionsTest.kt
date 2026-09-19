package dev.jux.intellij.run

import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.execution.configurations.RuntimeConfigurationError
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.xmlb.XmlSerializer
import org.jdom.Element

/**
 * The run configuration's build-system options: `--profile`, `--example`
 * and the doc-examples mode survive the run-configuration XML, and a mode
 * that needs a project refuses to start without a `jux.toml`.
 */
class JuxRunConfigurationBuildOptionsTest : BasePlatformTestCase() {

    private fun newConfig(): JuxRunConfiguration {
        val type = ConfigurationTypeUtil.findConfigurationType(JuxRunConfigurationType::class.java)
        return type.configurationFactories.first().createTemplateConfiguration(project) as JuxRunConfiguration
    }

    fun testOptionsRoundTripThroughXml() {
        val config = newConfig().apply {
            mode = JuxRunConfiguration.MODE_DOCTEST
            profile = "fast"
            example = "shapes"
            filePath = "/p/jux.toml"
        }
        // The platform persists run-configuration options as their state
        // object, serialized the way the workspace file stores it.
        val element: Element = XmlSerializer.serialize(config.options)
        val copy = newConfig()
        XmlSerializer.deserializeInto(copy.options, element)
        assertEquals(com.intellij.openapi.util.JDOMUtil.write(element), JuxRunConfiguration.MODE_DOCTEST, copy.mode)
        assertTrue(copy.isTestMode())
        assertTrue(copy.isDocTestMode())
        assertEquals("fast", copy.profile)
        assertEquals("shapes", copy.example)
    }

    fun testPlainTestModeIsNotDocMode() {
        val config = newConfig().apply { mode = JuxRunConfiguration.MODE_TEST }
        assertTrue(config.isTestMode())
        assertFalse(config.isDocTestMode())
    }

    fun testExampleRunNeedsAProject() {
        val config = newConfig().apply {
            filePath = "/definitely/not/here/a.jux"
            example = "shapes"
        }
        try {
            config.checkConfiguration()
            fail("an example run without a jux.toml must be rejected")
        } catch (e: RuntimeConfigurationError) {
            assertTrue(e.localizedMessage, e.localizedMessage.contains("needs a Jux project"))
        }
    }
}
