public class Main {
    static String grade(int score) {
        String r = "none";
        switch (score) {
            case 3 -> r = "high";
            case 2 -> r = "mid";
            default -> r = "low";
        }
        return r;
    }

    static String kind(String s) {
        String r = "none";
        switch (s) {
            case "a" -> r = "first";
            case "b" -> r = "second";
            default -> r = "other";
        }
        return r;
    }

    public static void main(String[] args) {
        System.out.println(grade(3));
        System.out.println(grade(2));
        System.out.println(grade(1));
        System.out.println(kind("a"));
        System.out.println(kind("b"));
        System.out.println(kind("z"));
        outer:
        for (int i = 0; i < 4; i++) {
            for (int j = 0; j < 4; j++) {
                if (j == 2) {
                    continue outer;
                }
                if (i == 3) {
                    break outer;
                }
                System.out.println(i * 10 + j);
            }
        }
        int n = 0;
        while (true) {
            n = n + 1;
            if (n > 3) {
                break;
            }
        }
        System.out.println(n);
    }
}
