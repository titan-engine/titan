<?php
namespace Extensions\Ian\Test\Http\Controllers;

use App\Http\Controllers\Controller;
use Illuminate\View\View;

class AdminController extends Controller {

    public function index(): View {
        return view('ian.test::admin.index');
    }

}
