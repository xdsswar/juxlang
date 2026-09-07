public class Main {
    static int fib(int n) {
        if (n < 2) { return n; }
        return fib(n - 1) + fib(n - 2);
    }
    static int gcd(int a, int b) {
        if (b == 0) { return a; }
        return gcd(b, a % b);
    }
    static int ackermannish(int m, int n) {
        if (m == 0) { return n + 1; }
        if (n == 0) { return ackermannish(m - 1, 1); }
        return ackermannish(m - 1, ackermannish(m, n - 1));
    }

    public static void main(String[] args) {
        System.out.println(fib(20));
        System.out.println(gcd(1071, 462));
        System.out.println(ackermannish(2, 3));
        int sum = 0;
        for (int i = 1; i <= 100; i++) { sum = sum + i; }
        System.out.println(sum);
    }
}
