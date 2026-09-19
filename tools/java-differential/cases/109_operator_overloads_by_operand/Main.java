public class Main {
    static class Vec2 {
        double x;
        double y;

        Vec2(double x, double y) {
            this.x = x;
            this.y = y;
        }

        Vec2 scale(double k) {
            return new Vec2(x * k, y * k);
        }

        double dot(Vec2 other) {
            return x * other.x + y * other.y;
        }

        Vec2 plus(Vec2 other) {
            return new Vec2(x + other.x, y + other.y);
        }
    }

    static class Point extends Vec2 {
        Point(double x, double y) {
            super(x, y);
        }
    }

    record Money(long cents) {
        Money plus(Money other) {
            return new Money(cents + other.cents);
        }

        Money plusCents(long more) {
            return new Money(cents + more);
        }
    }

    public static void main(String[] args) {
        Vec2 a = new Vec2(1.5, -2.0);
        Vec2 b = new Vec2(4.0, 0.5);
        Vec2 s = a.scale(3.0);
        System.out.println(s.x);
        System.out.println(s.y);
        System.out.println(a.dot(b));
        System.out.println(a.plus(b).dot(a));
        Vec2 acc = b;
        acc = acc.scale(2.0);
        acc = acc.scale(0.25);
        System.out.println(acc.x);
        System.out.println(acc.y);
        Point p = new Point(2.0, 3.0);
        System.out.println(p.dot(b));
        System.out.println(p.scale(2.0).y);

        Money m = new Money(700);
        m = m.plusCents(45L);
        m = m.plus(new Money(255));
        System.out.println(m.cents);
        System.out.println(m.plusCents(1L).plus(new Money(-1000)).cents);
    }
}
