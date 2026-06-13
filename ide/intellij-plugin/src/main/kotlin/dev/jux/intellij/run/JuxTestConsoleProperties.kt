package dev.jux.intellij.run

import com.intellij.execution.Executor
import com.intellij.execution.testframework.TestConsoleProperties
import com.intellij.execution.testframework.sm.SMCustomMessagesParsing
import com.intellij.execution.testframework.sm.runner.OutputToGeneralTestEventsConverter
import com.intellij.execution.testframework.sm.runner.SMTRunnerConsoleProperties
import com.intellij.execution.testframework.sm.runner.SMTestLocator

/**
 * SM-runner console wiring for `jux test`: plugs the §TS.7 output translator
 * ([JuxTestEventsConverter]) and the node→source locator ([JuxTestLocator])
 * into the standard test-tree console.
 */
class JuxTestConsoleProperties(
    config: JuxRunConfiguration,
    executor: Executor,
) : SMTRunnerConsoleProperties(config, FRAMEWORK_NAME, executor), SMCustomMessagesParsing {

    override fun createTestEventsConverter(
        testFrameworkName: String,
        consoleProperties: TestConsoleProperties,
    ): OutputToGeneralTestEventsConverter = JuxTestEventsConverter(testFrameworkName, consoleProperties)

    override fun getTestLocator(): SMTestLocator = JuxTestLocator

    companion object {
        const val FRAMEWORK_NAME = "Jux Test"
    }
}
