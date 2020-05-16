<?php
namespace Extensions\Ian\Test\Http\Controllers;

use App\Http\Controllers\Controller;
use Illuminate\View\View;

class FrontController extends Controller {

    public function index(): View {
        return view('ian.test::index');
    }

}
