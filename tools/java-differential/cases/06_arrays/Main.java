public class Main {
    static void fill(int[] xs) {
        for (int i = 0; i < xs.length; i++) {
            xs[i] = i * i;
        }
    }

    public static void main(String[] args) {
        int[] a = new int[5];
        System.out.println(a.length);
        System.out.println(a[0]);
        fill(a);
        System.out.println(a[4]);
        int[] b = a;
        b[0] = 99;
        System.out.println(a[0]);

        int[][] grid = new int[3][4];
        grid[1][2] = 7;
        System.out.println(grid.length);
        System.out.println(grid[0].length);
        System.out.println(grid[1][2]);
        System.out.println(grid[0][0]);

        int total = 0;
        for (int v : a) {
            total = total + v;
        }
        System.out.println(total);

        String[] names = new String[2];
        names[0] = "x";
        System.out.println(names[0]);
    }
}
