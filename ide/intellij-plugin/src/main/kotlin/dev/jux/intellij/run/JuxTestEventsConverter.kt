package dev.jux.intellij.run

import com.intellij.execution.testframework.TestConsoleProperties
import com.intellij.execution.testframework.sm.ServiceMessageBuilder
import com.intellij.execution.testframework.sm.runner.OutputToGeneralTestEventsConverter
import com.intellij.openapi.util.Key
import jetbrains.buildServer.messages.serviceMessages.ServiceMessageVisitor

/**
 * Translates `jux test` stdout (§TS.7), and the doc-example report of
 * `jux test --doc` (§12.5), into the SM test-runner protocol that builds the
 * green/red test tree. Line classification lives in the pure
 * [JuxTestOutputParser]; this class only maps classified lines to service
 * messages.
 *
 * The runner prints each test's status AFTER it finished (tests run
 * sequentially, §TS.2), so started/failed/finished events are emitted together
 * per status line: the tree fills in as each test completes. Everything
 * non-status passes through untouched, so raw compiler/program output still
 * shows in the console.
 *
 * A failed doc example is the one exception: its reason follows the `FAIL`
 * line, indented seven spaces (the compiler's errors, or the program's
 * output). The node stays open until those lines are read, so the reason
 * becomes the failure message the tree shows beside the test.
 */
class JuxTestEventsConverter(
    testFrameworkName: String,
    consoleProperties: TestConsoleProperties,
) : OutputToGeneralTestEventsConverter(testFrameworkName, consoleProperties) {

    /** A failed doc example whose reason lines are still arriving. */
    private var pendingDocFail: Pair<String, StringBuilder>? = null

    override fun processServiceMessages(text: String, outputType: Key<*>, visitor: ServiceMessageVisitor): Boolean {
        lastVisitor = visitor
        pendingDocFail?.let { (_, reason) ->
            if (text.startsWith(DOC_REASON_INDENT)) {
                reason.append(text.trimEnd('\r', '\n').removePrefix(DOC_REASON_INDENT)).append('\n')
                return true
            }
            finishPendingDocFail(outputType, visitor)
        }
        when (val line = JuxTestOutputParser.classifyDoc(text)) {
            is JuxTestOutputParser.Line.RunStart -> {
                // Progress bar denominator; keep the raw line in the console too.
                emit(ServiceMessageBuilder("testCount").addAttribute("count", line.count.toString()), outputType, visitor)
                return super.processServiceMessages(text, outputType, visitor)
            }
            is JuxTestOutputParser.Line.DocRunStart -> {
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
            is JuxTestOutputParser.Line.DocPass -> {
                emit(docStarted(line.owner, line.location), outputType, visitor)
                emit(ServiceMessageBuilder.testFinished(line.owner), outputType, visitor)
                return true
            }
            is JuxTestOutputParser.Line.DocFail -> {
                emit(docStarted(line.owner, line.location), outputType, visitor)
                pendingDocFail = line.owner to StringBuilder()
                return true
            }
            // Summaries and everything else: plain console output.
            else -> return super.processServiceMessages(text, outputType, visitor)
        }
    }

    /** The process ended: a still-open doc failure is closed with what it has. */
    override fun flushBufferOnProcessTermination(exitCode: Int) {
        lastVisitor?.let { finishPendingDocFail(com.intellij.execution.process.ProcessOutputTypes.STDOUT, it) }
        super.flushBufferOnProcessTermination(exitCode)
    }

    /** The visitor of the latest line, reused to close a doc failure at exit. */
    private var lastVisitor: ServiceMessageVisitor? = null

    /** Close the open doc failure with the reason collected so far. */
    private fun finishPendingDocFail(outputType: Key<*>, visitor: ServiceMessageVisitor) {
        val (name, reason) = pendingDocFail ?: return
        pendingDocFail = null
        emit(
            ServiceMessageBuilder.testFailed(name)
                .addAttribute("message", reason.toString().trim().ifEmpty { "doc example failed" }),
            outputType,
            visitor,
        )
        emit(ServiceMessageBuilder.testFinished(name), outputType, visitor)
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

    /**
     * A doc example's node: named after the item it documents, located at
     * that item's declaration (`jux:doctest://file:line`).
     */
    private fun docStarted(owner: String, location: String): ServiceMessageBuilder =
        ServiceMessageBuilder.testStarted(owner)
            .addAttribute("locationHint", "${JuxTestLocator.DOC_PROTOCOL}://$location")

    /** Route a built service message through the inherited TeamCity parser. */
    private fun emit(builder: ServiceMessageBuilder, outputType: Key<*>, visitor: ServiceMessageVisitor) {
        super.processServiceMessages(builder.toString() + "\n", outputType, visitor)
    }

    private companion object {
        /** How `jux test --doc` indents a failed example's reason lines. */
        const val DOC_REASON_INDENT = "       "
    }
}
