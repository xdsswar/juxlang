public class Main {
    static String classify(int n) {
        switch (n) {
            case 0 -> { return "zero"; }
            case 1 -> { return "one"; }
            case 2 -> { return "two"; }
            default -> { return "many"; }
        }
    }

    public static void main(String[] args) {
        outer:
        for (int i = 0; i < 4; i++) {
            for (int j = 0; j < 4; j++) {
                if (j == 2) { continue outer; }
                if (i == 3) { break outer; }
                System.out.println(i + ":" + j);
            }
        }

        int n = 0;
        do {
            System.out.println("do " + n);
            n = n + 1;
        } while (n < 3);

        int w = 5;
        while (w > 0) {
            w = w - 2;
        }
        System.out.println(w);

        for (int i = 0; i < 5; i++) {
            System.out.println(classify(i));
        }
    }
}
