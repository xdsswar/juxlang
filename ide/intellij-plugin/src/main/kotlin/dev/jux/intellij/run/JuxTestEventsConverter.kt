package dev.jux.intellij.run

import com.intellij.execution.testframework.TestConsoleProperties
import com.intellij.execution.testframework.sm.ServiceMessageBuilder
import com.intellij.execution.testframework.sm.runner.OutputToGeneralTestEventsConverter
import com.intellij.openapi.util.Key
import jetbrains.buildServer.messages.serviceMessages.ServiceMessageVisitor

/**
 * Translates `jux test` stdout (§TS.7) into the SM test-runner protocol that
 * builds the green/red test tree. Line classification lives in the pure
 * [JuxTestOutputParser]; this class only maps classified lines to service
 * messages.
 *
 * The runner prints each test's status AFTER it finished (tests run
 * sequentially, §TS.2), so started/failed/finished events are emitted together
 * per status line — the tree fills in as each test completes. Everything
 * non-status passes through untouched, so raw compiler/program output still
 * shows in the console.
 */
class JuxTestEventsConverter(
    testFrameworkName: String,
    consoleProperties: TestConsoleProperties,
) : OutputToGeneralTestEventsConverter(testFrameworkName, consoleProperties) {

    override fun processServiceMessages(text: String, outputType: Key<*>, visitor: ServiceMessageVisitor): Boolean {
        when (val line = JuxTestOutputParser.classify(text)) {
            is JuxTestOutputParser.Line.RunStart -> {
                // Progress bar denominator; keep the raw line in the console too.
                emit(ServiceMessageBuilder("testCount").addAttribute("count", line.count.toString()), outputType, visitor)
                return super.processServiceMessages(text, outputType, visitor)
            }
            is JuxTestOutputParser.Line.Pass -> {
                emit(testStarted(line.name), outputType, visitor)
                emit(ServiceMessageBuilder.testFinished(line.name), outputType, visitor)
                return true
            }
            is JuxTestOutputParser.Line.Fail -> {
                emit(testStarted(line.name), outputType, visitor)
                emit(
                    ServiceMessageBuilder.testFailed(line.name).addAttribute("message", line.message),
                    outputType,
                    visitor,
                )
                emit(ServiceMessageBuilder.testFinished(line.name), outputType, visitor)
                return true
            }
            // Summary and everything else: plain console output.
            else -> return super.processServiceMessages(text, outputType, visitor)
        }
    }

    /**
     * A testStarted message with a location hint so double-click / jump-to-source
     * resolves through [JuxTestLocator] (`jux:test://pkg.fn`). The synthetic
     * `<afterAll>` hook node gets no hint — there is no single function to open.
     */
    private fun testStarted(name: String): ServiceMessageBuilder {
        val b = ServiceMessageBuilder.testStarted(name)
        if (!name.startsWith("<")) {
            b.addAttribute("locationHint", "${JuxTestLocator.PROTOCOL}://$name")
        }
        return b
    }

    /** Route a built service message through the inherited TeamCity parser. */
    private fun emit(builder: ServiceMessageBuilder, outputType: Key<*>, visitor: ServiceMessageVisitor) {
        super.processServiceMessages(builder.toString() + "\n", outputType, visitor)
    }
}
