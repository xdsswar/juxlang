public class Main {
    static class Err extends Exception {
        Err(String m) { super(m); }
    }

    static int plainReturn() {
        try {
            return 1;
        } finally {
            System.out.println("f1");
        }
    }

    static int catchReturns() {
        try {
            throw new Err("x");
        } catch (Err e) {
            return 2;
        } finally {
            System.out.println("f2");
        }
    }

    static int breakThroughFinally() {
        int total = 0;
        for (int i = 0; i < 4; i++) {
            try {
                if (i == 2) { break; }
                total = total + i;
            } finally {
                total = total + 10;
            }
        }
        return total;
    }

    static String nested() {
        try {
            try {
                throw new Err("inner");
            } finally {
                System.out.println("inner-finally");
            }
        } catch (Err e) {
            return "caught " + e.getMessage();
        }
    }

    public static void main(String[] args) {
        System.out.println(plainReturn());
        System.out.println(catchReturns());
        System.out.println(breakThroughFinally());
        System.out.println(nested());
    }
}
