public class Main {
    static class Acc {
        int total;
        Acc(int total) { this.total = total; }
    }

    public static void main(String[] args) {
        int a = 29;
        a += 3;
        System.out.println(a);
        a -= 2;
        System.out.println(a);
        a *= 4;
        System.out.println(a);
        a /= 5;
        System.out.println(a);
        a %= 7;
        System.out.println(a);

        int bits = 12;
        bits &= 10;
        System.out.println(bits);
        bits |= 5;
        System.out.println(bits);
        bits ^= 3;
        System.out.println(bits);
        bits <<= 4;
        System.out.println(bits);
        bits >>= 2;
        System.out.println(bits);

        int neg = -64;
        neg >>= 3;
        System.out.println(neg);

        long big = 1;
        big <<= 40;
        System.out.println(big);
        big >>= 10;
        System.out.println(big);

        double d = 10.0;
        d /= 4.0;
        System.out.println(d);
        d *= 3.0;
        System.out.println(d);

        String s = "a";
        s += "b";
        s += 1;
        System.out.println(s);

        Acc acc = new Acc(10);
        acc.total += 5;
        acc.total *= 2;
        System.out.println(acc.total);

        int[] xs = new int[3];
        xs[0] = 4;
        xs[0] += 6;
        xs[0] <<= 1;
        xs[0] %= 7;
        System.out.println(xs[0]);

        java.util.List<Integer> v = new java.util.ArrayList<>();
        v.add(3);
        v.set(0, v.get(0) + 9);
        System.out.println(v.get(0));
    }
}
