public class Main {
    static class AppError extends Exception {
        AppError(String m) { super(m); }
    }

    static int risky(int n) throws AppError {
        if (n < 0) {
            throw new AppError("negative");
        }
        return n * 2;
    }

    public static void main(String[] args) {
        try {
            System.out.println(risky(5));
            System.out.println(risky(-1));
            System.out.println("unreachable");
        } catch (AppError e) {
            System.out.println("caught " + e.getMessage());
        } finally {
            System.out.println("finally 1");
        }

        for (int i = 0; i < 3; i++) {
            try {
                if (i == 1) { continue; }
                if (i == 2) { break; }
                System.out.println("body " + i);
            } finally {
                System.out.println("fin " + i);
            }
        }

        try {
            try {
                throw new AppError("inner");
            } finally {
                System.out.println("inner finally");
            }
        } catch (AppError e) {
            System.out.println("outer caught " + e.getMessage());
        }
        System.out.println("done");
    }
}
