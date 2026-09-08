public class Main {
    static class AppError extends Exception {
        AppError(String m) { super(m); }
    }
    static class DiskError extends AppError {
        DiskError(String m) { super(m); }
    }

    static String classify(int code) {
        try {
            if (code == 1) {
                throw new DiskError("disk");
            }
            if (code == 2) {
                throw new AppError("app");
            }
            return "ok";
        } catch (DiskError e) {
            return "disk:" + e.getMessage();
        } catch (AppError e) {
            return "app:" + e.getMessage();
        } finally {
            System.out.println("finally " + code);
        }
    }

    static int counted(int code) {
        int n = 0;
        try {
            n = n + 1;
            if (code > 0) {
                throw new AppError("boom");
            }
            n = n + 10;
        } catch (AppError e) {
            n = n + 100;
        } finally {
            n = n + 1000;
        }
        return n;
    }

    public static void main(String[] args) throws Exception {
        System.out.println(classify(0));
        System.out.println(classify(1));
        System.out.println(classify(2));
        System.out.println(counted(0));
        System.out.println(counted(1));
        try {
            try {
                throw new DiskError("inner");
            } finally {
                System.out.println("inner finally");
            }
        } catch (AppError e) {
            System.out.println("outer caught " + e.getMessage());
        }
    }
}
