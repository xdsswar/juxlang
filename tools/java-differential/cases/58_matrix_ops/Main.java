public class Main {
    static int[][] multiply(int[][] a, int[][] b, int n) {
        int[][] out = new int[n][n];
        for (int i = 0; i < n; i++) {
            for (int j = 0; j < n; j++) {
                int sum = 0;
                for (int k = 0; k < n; k++) {
                    sum = sum + a[i][k] * b[k][j];
                }
                out[i][j] = sum;
            }
        }
        return out;
    }

    static void show(int[][] m, int n) {
        for (int i = 0; i < n; i++) {
            String row = "";
            for (int j = 0; j < n; j++) {
                if (j > 0) {
                    row = row + ",";
                }
                row = row + m[i][j];
            }
            System.out.println(row);
        }
    }

    public static void main(String[] args) {
        int[][] a = new int[2][2];
        a[0][0] = 1;
        a[0][1] = 2;
        a[1][0] = 3;
        a[1][1] = 4;
        int[][] b = new int[2][2];
        b[0][0] = 5;
        b[0][1] = 6;
        b[1][0] = 7;
        b[1][1] = 8;
        show(multiply(a, b, 2), 2);
        show(a, 2);

        int[][] grid = new int[2][3];
        grid[0][0] = 9;
        grid[0][1] = 8;
        grid[0][2] = 7;
        grid[1][0] = grid[0][2];
        grid[0][1] = grid[0][0];
        show(grid, 2);
    }
}
