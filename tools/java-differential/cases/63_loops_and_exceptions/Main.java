public class Main {
    static int scan(int limit) {
        int seen = 0;
        int i = 0;
        do {
            i = i + 1;
            if (i % 2 == 0) {
                continue;
            }
            seen = seen + i;
        } while (i < limit);
        return seen;
    }

    static String risky(int i) {
        if (i == 2) {
            throw new RuntimeException("two");
        }
        return "v" + i;
    }

    static int guarded(int n) {
        int ok = 0;
        for (int i = 0; i < n; i++) {
            try {
                System.out.println(risky(i));
                ok = ok + 1;
            } catch (RuntimeException e) {
                System.out.println("caught " + e.getMessage());
                continue;
            } finally {
                System.out.println("step " + i);
            }
        }
        return ok;
    }

    static int breaker(int n) {
        int total = 0;
        for (int i = 0; i < n; i++) {
            try {
                if (i == 3) {
                    break;
                }
                total = total + i;
            } finally {
                System.out.println("fin " + i);
            }
        }
        return total;
    }

    public static void main(String[] args) {
        System.out.println(scan(5));
        System.out.println(guarded(4));
        System.out.println(breaker(5));
    }
}
