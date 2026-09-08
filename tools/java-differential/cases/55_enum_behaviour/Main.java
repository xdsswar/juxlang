public class Main {
    enum Level {
        Low,
        Mid,
        High;

        int weight() {
            return switch (this) {
                case Low -> 1;
                case Mid -> 5;
                case High -> 10;
            };
        }

        String label() {
            return switch (this) {
                case Low -> "low";
                case Mid -> "mid";
                case High -> "high";
            };
        }
    }

    static int totalOf(Level a, Level b) {
        return a.weight() + b.weight();
    }

    public static void main(String[] args) {
        System.out.println(Level.Low.weight());
        System.out.println(Level.High.weight());
        System.out.println(Level.Mid.label());
        System.out.println(totalOf(Level.Low, Level.High));
        Level l = Level.Mid;
        if (l == Level.Mid) {
            System.out.println("is mid");
        }
        if (l != Level.High) {
            System.out.println("not high");
        }
        switch (l) {
            case Mid -> System.out.println("switched mid");
            default -> System.out.println("switched other");
        }
    }
}
