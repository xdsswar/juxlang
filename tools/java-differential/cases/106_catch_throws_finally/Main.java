public class Main {
    static class Fault extends RuntimeException {
        Fault(String m) { super(m); }
    }

    static class DeepFault extends Fault {
        DeepFault(String m) { super(m); }
    }

    static void boom(String why) { throw new IllegalArgumentException(why); }

    static String viaThrowExpr(String s) {
        try {
            throw new IllegalStateException("first");
        } catch (IllegalStateException e) {
            System.out.println("catch expr");
            if (s == null) throw new IllegalArgumentException("expr");
            return s;
        } finally {
            System.out.println("finally expr");
        }
    }

    static String viaCall() {
        try {
            throw new IllegalStateException("first");
        } catch (IllegalStateException e) {
            System.out.println("catch call");
            boom("call");
            return "unreached";
        } finally {
            System.out.println("finally call");
        }
    }

    static void viaStatement() {
        try {
            throw new IllegalStateException("first");
        } catch (IllegalStateException e) {
            System.out.println("catch stmt");
            throw new IllegalArgumentException("stmt");
        } finally {
            System.out.println("finally stmt");
        }
    }

    static void rethrowSubclass() {
        try {
            throw new DeepFault("deep");
        } catch (Fault f) {
            System.out.println("catch base " + f.getMessage());
            throw f;
        } finally {
            System.out.println("finally rethrow");
        }
    }

    static int countedThenThrow(int[] log) {
        int steps = 0;
        try {
            try {
                throw new IllegalStateException("first");
            } catch (IllegalStateException e) {
                steps = steps + 1;
                log[0] = steps;
                boom("counted");
                steps = steps + 100;
            } finally {
                steps = steps + 10;
                log[1] = steps;
            }
        } catch (IllegalArgumentException e) {
            System.out.println("outer counted " + e.getMessage());
        }
        return steps;
    }

    static int returnOrThrow(boolean ok) {
        try {
            throw new IllegalStateException("first");
        } catch (IllegalStateException e) {
            if (ok) {
                return 7;
            }
            boom("either");
            return -1;
        } finally {
            System.out.println("finally either " + ok);
        }
    }

    public static void main(String[] args) {
        try { System.out.println(viaThrowExpr(null)); } catch (IllegalArgumentException e) { System.out.println("outer " + e.getMessage()); }
        System.out.println(viaThrowExpr("kept"));
        try { System.out.println(viaCall()); } catch (IllegalArgumentException e) { System.out.println("outer " + e.getMessage()); }
        try { viaStatement(); } catch (IllegalArgumentException e) { System.out.println("outer " + e.getMessage()); }
        try { rethrowSubclass(); } catch (DeepFault d) { System.out.println("outer deep " + d.getMessage()); }
        int[] log = new int[2];
        System.out.println(countedThenThrow(log));
        System.out.println(log[0]);
        System.out.println(log[1]);
        System.out.println(returnOrThrow(true));
        try { System.out.println(returnOrThrow(false)); } catch (IllegalArgumentException e) { System.out.println("outer " + e.getMessage()); }
    }
}
