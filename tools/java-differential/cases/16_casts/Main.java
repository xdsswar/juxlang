public class Main {
    public static void main(String[] args) {
        double d = 9.99;
        System.out.println((int) d);
        System.out.println((int) -9.99);
        System.out.println((long) 3.7);

        int big = 300;
        System.out.println((byte) big);
        System.out.println((short) 70000);

        char c = 'A';
        System.out.println((int) c);
        System.out.println((char) 66);
        System.out.println((int) 'z');

        int i = 7;
        double back = (double) i;
        System.out.println(back);
        System.out.println((double) 7 / (double) 2);
        System.out.println(7 / 2);

        long l = 5;
        int narrowed = (int) l;
        System.out.println(narrowed);
    }
}
