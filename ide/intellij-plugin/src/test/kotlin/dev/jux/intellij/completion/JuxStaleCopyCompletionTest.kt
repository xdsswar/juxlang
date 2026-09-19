package dev.jux.intellij.completion

import com.intellij.codeInsight.lookup.LookupManager
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Completing twice at the same place in a file whose text was loaded again.
 *
 * The random completion sweep found that the second completion threw
 * `PsiInvalidElementAccessException`: completion's non-physical file copy
 * outlived the tree it was copied from, and a type cached on it still named a
 * field of the old tree. Types that name dead PSI are now recomputed.
 */
class JuxStaleCopyCompletionTest : BasePlatformTestCase() {

    private val code = """
        class Box {
            public String? label = null;
        }

        public void main() {
            var b = new Box();
            b.label = "xy";
            print(b.label?.len<caret>());
        }
    """.trimIndent()

    fun testCompletingAgainAfterTheFileIsReloaded() {
        repeat(3) {
            myFixture.configureByText("stale.jux", code)
            myFixture.completeBasic()
            LookupManager.getInstance(project).hideActiveLookup()
            PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
        }
    }
}
