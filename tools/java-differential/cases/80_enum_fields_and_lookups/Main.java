public class Main {
    enum Planet {
        Mercury(3.303e+23, 2.4397e6),
        Venus(4.869e+24, 6.0518e6),
        Earth(5.976e+24, 6.37814e6);

        private final double mass;
        private final double radius;

        Planet(double mass, double radius) {
            this.mass = mass;
            this.radius = radius;
        }

        public double gravity() {
            return 6.67300E-11 * mass / (radius * radius);
        }
    }

    enum Op {
        Add("+", 1),
        Mul("*", 2);

        public final String symbol;
        public final int precedence;

        Op(String symbol, int precedence) {
            this.symbol = symbol;
            this.precedence = precedence;
        }

        public int apply(int a, int b) {
            return switch (this) {
                case Add -> a + b;
                case Mul -> a * b;
            };
        }
    }

    static Planet lookup(String name) {
        try {
            return Planet.valueOf(name);
        } catch (IllegalArgumentException e) {
            return null;
        }
    }

    public static void main(String[] args) {
        for (Planet p : Planet.values()) {
            long g = (long) (p.gravity() * 1000.0);
            System.out.println(p.name() + " " + p.ordinal() + " " + g);
        }
        for (Op op : Op.values()) {
            System.out.println(op.symbol + " " + op.precedence + " " + op.apply(6, 7));
        }
        Planet found = lookup("Venus");
        System.out.println(found == null ? "missing" : found.name());
        Planet missing = lookup("Pluto");
        System.out.println(missing == null ? "missing" : missing.name());
        System.out.println(Op.values().length > 1 ? Op.values()[1].symbol : "none");
    }
}
