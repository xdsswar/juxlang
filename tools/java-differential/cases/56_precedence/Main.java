public class Main {
    public static void main(String[] args) {
        System.out.println(2 + 3 * 4);
        System.out.println((2 + 3) * 4);
        System.out.println(10 - 4 - 3);
        System.out.println(100 / 10 / 2);
        System.out.println(2 + 3 > 4);
        System.out.println(1 < 2 == true);
        System.out.println(!(1 > 2));
        System.out.println(1 + 2 == 3 && 4 > 3);
        System.out.println(true || false && false);
        System.out.println((true || false) && false);
        System.out.println(5 % 3 * 2);
        System.out.println(-3 + 4);
        System.out.println(- (3 + 4));
        System.out.println(1 << 2 + 1);
        System.out.println((1 << 2) + 1);
        System.out.println(7 & 3 | 4);
        System.out.println(7 & (3 | 4));
        int a = 2;
        int b = 3;
        int c = 4;
        System.out.println(a + b * c - a);
        System.out.println(a * b % c);
        System.out.println(a < b == b < c);
        boolean t = a < b;
        System.out.println(t ? "lt" : "ge");
        System.out.println(a == 2 ? b : c);
    }
}
